//! Complete restored funding-intent prerequisites, not observed historical admissions.
use super::proposal::PreparedProvider;
use chiral_network::{
    chain_client::ValidatedPaymentEnvelope,
    protocol::{self, FundingIntent, Quote, Signed},
    wallet,
};
use rusqlite::{params, Connection};

pub(super) fn restore_retained_intents(
    db: &mut Connection, provider: &PreparedProvider, template: &Signed<Quote>, count: u64,
    original: impl Fn(&Quote, &str, u64) -> (String, ValidatedPaymentEnvelope),
) {
    // Callers independently sign and validate each envelope before insertion.
    // Reuse accepted timestamps; never extend a signed term or an admission lease.
    let retained_key = format!("{:064x}", 123_456);
    let retained_payer = wallet::address_from_private_key(&retained_key).unwrap();
    assert_ne!(retained_payer, template.payload.consumer_wallet);
    let tx = db.transaction().unwrap();
    {
        let mut insert = tx.prepare(
            "INSERT INTO funding_intents(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)").unwrap();
        for sequence in 1..=count {
            let mut payload = template.payload.clone();
            payload.id = format!("0x{:064x}", 0xfeed_0000 + sequence);
            payload.nonce = format!("0x{:064x}", 0xcafe_0000 + sequence);
            payload.consumer_wallet = retained_payer.clone();
            let quote = protocol::sign(protocol::QUOTE_DOMAIN, payload, &provider.state.private_key).unwrap();
            let (raw, envelope) = original(&quote.payload, &retained_key, sequence);
            let ack = protocol::sign(protocol::INTENT_DOMAIN, FundingIntent {
                network: quote.payload.network.clone(), provider_wallet: quote.payload.provider_wallet.clone(),
                consumer_wallet: retained_payer.clone(), quote_id: quote.payload.id.clone(),
                tx_hash: envelope.tx_hash.clone(), registered_at: quote.payload.issued_at,
            }, &provider.state.private_key).unwrap();
            insert.execute(params![quote.payload.id, envelope.tx_hash, raw,
                serde_json::to_string(&ack).unwrap(), serde_json::to_string(&quote).unwrap(),
                retained_payer, sequence.to_string(), quote.payload.expires_at]).unwrap();
        }
    }
    tx.commit().unwrap();
}
