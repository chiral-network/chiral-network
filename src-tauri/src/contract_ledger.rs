//! Provider-side contract ledger — the prepaid-balance accounting for the
//! resource exchange. A provider opens a contract from a verified funding
//! transaction, draws the balance down as it meters usage, and accepts
//! non-refundable top-ups. A spent-transaction guard ensures each funding /
//! top-up tx is applied exactly once (no replay).
//!
//! Design: `docs/chiral-book.md` — "Service Contracts and the Handshake" →
//! "Balance, metering, and top-ups". The platform-fee cut reuses
//! `speed_tiers::split_payment` (single source of truth; `credit + fee == total`
//! exactly, integer math).
//!
//! This is the in-memory accounting core; on-chain verification of the funding
//! tx (recipient / amount / chain-id) and persistence are layered on by the
//! callers (the handshake handlers), not here.

use std::collections::{HashMap, HashSet};

use crate::resource_offer::ResourceClass;
use crate::speed_tiers::split_payment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractStatus {
    Open,
    Paused,
    Closed,
}

#[derive(Debug, Clone)]
pub struct ContractState {
    /// `contract_id == funding tx hash` (lowercased).
    pub contract_id: String,
    pub consumer_wallet: String,
    pub resource_class: ResourceClass,
    /// Gross CHI paid in (funding + top-ups), before the platform fee.
    pub funded_wei: u128,
    /// Net CHI credited to the spendable balance (after the fee cut).
    pub credited_wei: u128,
    /// CHI drawn down by metered usage.
    pub spent_wei: u128,
    pub status: ContractStatus,
}

impl ContractState {
    pub fn balance_wei(&self) -> u128 {
        self.credited_wei.saturating_sub(self.spent_wei)
    }
}

/// Tracks all contracts a provider has open, the transactions already applied,
/// and the platform fee accrued.
#[derive(Debug, Default)]
pub struct ContractLedger {
    contracts: HashMap<String, ContractState>,
    spent_tx: HashSet<String>,
    platform_fee_wei: u128,
}

impl ContractLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a contract from a verified funding tx (`contract_id == tx_hash`).
    /// The caller must have already verified the tx on-chain. Errors if the
    /// contract already exists or the funding tx was already applied.
    pub fn open(
        &mut self,
        contract_id: &str,
        consumer_wallet: &str,
        resource_class: ResourceClass,
        amount_wei: u128,
    ) -> Result<(), String> {
        let id = contract_id.to_lowercase();
        if self.contracts.contains_key(&id) {
            return Err("contract already open".to_string());
        }
        if !self.spent_tx.insert(id.clone()) {
            return Err("funding tx already applied".to_string());
        }
        let (credit, fee) = split_payment(amount_wei);
        self.platform_fee_wei = self.platform_fee_wei.saturating_add(fee);
        self.contracts.insert(
            id.clone(),
            ContractState {
                contract_id: id,
                consumer_wallet: consumer_wallet.to_lowercase(),
                resource_class,
                funded_wei: amount_wei,
                credited_wei: credit,
                spent_wei: 0,
                status: ContractStatus::Open,
            },
        );
        Ok(())
    }

    /// Apply a non-refundable top-up. `tx_hash` must be distinct from every tx
    /// already applied (funding or prior top-ups). Re-opens a paused contract.
    pub fn topup(
        &mut self,
        contract_id: &str,
        tx_hash: &str,
        amount_wei: u128,
    ) -> Result<(), String> {
        let id = contract_id.to_lowercase();
        let tx = tx_hash.to_lowercase();
        match self.contracts.get(&id).map(|c| c.status) {
            None => return Err("unknown contract".to_string()),
            Some(ContractStatus::Closed) => return Err("contract closed".to_string()),
            _ => {}
        }
        if !self.spent_tx.insert(tx) {
            return Err("top-up tx already applied".to_string());
        }
        let (credit, fee) = split_payment(amount_wei);
        self.platform_fee_wei = self.platform_fee_wei.saturating_add(fee);
        let c = self
            .contracts
            .get_mut(&id)
            .expect("existence checked above");
        c.funded_wei = c.funded_wei.saturating_add(amount_wei);
        c.credited_wei = c.credited_wei.saturating_add(credit);
        if c.status == ContractStatus::Paused && c.balance_wei() > 0 {
            c.status = ContractStatus::Open;
        }
        Ok(())
    }

    /// Draw down `cost_wei` of metered usage; returns the remaining balance.
    /// If the balance cannot cover the cost, nothing is drawn and the contract
    /// is paused — callers pre-authorize (worst-case) to avoid this.
    pub fn draw_down(&mut self, contract_id: &str, cost_wei: u128) -> Result<u128, String> {
        let c = self
            .contracts
            .get_mut(&contract_id.to_lowercase())
            .ok_or("unknown contract")?;
        if c.status == ContractStatus::Closed {
            return Err("contract closed".to_string());
        }
        if cost_wei > c.balance_wei() {
            c.status = ContractStatus::Paused;
            return Err("insufficient balance".to_string());
        }
        c.spent_wei = c.spent_wei.saturating_add(cost_wei);
        if c.balance_wei() == 0 {
            c.status = ContractStatus::Paused;
        }
        Ok(c.balance_wei())
    }

    pub fn get(&self, contract_id: &str) -> Option<&ContractState> {
        self.contracts.get(&contract_id.to_lowercase())
    }

    pub fn balance_wei(&self, contract_id: &str) -> Option<u128> {
        self.get(contract_id).map(|c| c.balance_wei())
    }

    pub fn platform_fee_wei(&self) -> u128 {
        self.platform_fee_wei
    }

    pub fn close(&mut self, contract_id: &str) -> Result<(), String> {
        let c = self
            .contracts
            .get_mut(&contract_id.to_lowercase())
            .ok_or("unknown contract")?;
        c.status = ContractStatus::Closed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_CHI: u128 = 1_000_000_000_000_000_000;
    const TX_A: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TX_B: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn open_one(l: &mut ContractLedger) {
        l.open(TX_A, "0xConsumer", ResourceClass::Storage, ONE_CHI)
            .expect("open");
    }

    #[test]
    fn open_credits_net_of_fee() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        let c = l.get(TX_A).unwrap();
        // 0.5% of 1 CHI = 5e15 fee; 995e15 credited.
        assert_eq!(c.funded_wei, ONE_CHI);
        assert_eq!(c.credited_wei, 995_000_000_000_000_000);
        assert_eq!(c.spent_wei, 0);
        assert_eq!(c.balance_wei(), 995_000_000_000_000_000);
        assert_eq!(c.credited_wei + l.platform_fee_wei(), ONE_CHI); // credit + fee == total
        assert_eq!(c.status, ContractStatus::Open);
    }

    #[test]
    fn double_open_and_replayed_tx_rejected() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        assert!(l.open(TX_A, "0xConsumer", ResourceClass::Storage, ONE_CHI).is_err());
    }

    #[test]
    fn draw_down_deducts_and_preserves_invariant() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        let cost = 100_000_000_000_000_000; // 0.1 CHI
        let remaining = l.draw_down(TX_A, cost).unwrap();
        let c = l.get(TX_A).unwrap();
        assert_eq!(c.spent_wei, cost);
        assert_eq!(remaining, c.balance_wei());
        assert_eq!(c.spent_wei + c.balance_wei(), c.credited_wei); // invariant
    }

    #[test]
    fn overdraw_pauses_and_draws_nothing() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        let before = l.balance_wei(TX_A).unwrap();
        assert!(l.draw_down(TX_A, ONE_CHI * 2).is_err());
        let c = l.get(TX_A).unwrap();
        assert_eq!(c.spent_wei, 0, "nothing drawn on insufficient balance");
        assert_eq!(l.balance_wei(TX_A).unwrap(), before);
        assert_eq!(c.status, ContractStatus::Paused);
    }

    #[test]
    fn topup_adds_credit_and_unpauses() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        // Drain to zero -> Paused.
        let bal = l.balance_wei(TX_A).unwrap();
        l.draw_down(TX_A, bal).unwrap();
        assert_eq!(l.get(TX_A).unwrap().status, ContractStatus::Paused);

        l.topup(TX_A, TX_B, ONE_CHI).unwrap();
        let c = l.get(TX_A).unwrap();
        assert_eq!(c.funded_wei, ONE_CHI * 2);
        assert_eq!(c.status, ContractStatus::Open);
        assert_eq!(c.balance_wei(), 995_000_000_000_000_000);
    }

    #[test]
    fn topup_rejects_reused_tx() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        // Reusing the funding tx as a top-up tx must fail.
        assert!(l.topup(TX_A, TX_A, ONE_CHI).is_err());
        l.topup(TX_A, TX_B, ONE_CHI).unwrap();
        assert!(l.topup(TX_A, TX_B, ONE_CHI).is_err()); // TX_B now used
    }

    #[test]
    fn closed_contract_refuses_activity() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        l.close(TX_A).unwrap();
        assert!(l.draw_down(TX_A, 1).is_err());
        assert!(l.topup(TX_A, TX_B, ONE_CHI).is_err());
    }

    #[test]
    fn fee_accumulates_across_contracts() {
        let mut l = ContractLedger::new();
        open_one(&mut l);
        l.open(TX_B, "0xOther", ResourceClass::Inference, ONE_CHI).unwrap();
        // Two 1-CHI fundings => 2 * 5e15 fee.
        assert_eq!(l.platform_fee_wei(), 10_000_000_000_000_000);
    }
}
