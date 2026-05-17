// Verified version of the chain-RPC proxy method allowlist.
//
// This file is the formal-verification companion to
// `src-tauri/src/chain_rpc_api.rs::is_allowed_method`. It's checked
// by Verus (https://verus-lang.github.io), not by `cargo`. To verify:
//
//   ~/.verus/verus-x86-linux/verus verified/method_allowlist.rs
//
// Goal — prove UNIVERSALLY (not example-by-example) that no JSON-RPC
// method name outside the four read-only namespaces can pass the
// allowlist. The existing example-based tests in chain_rpc_api.rs
// (`allows_read_only_namespaces`, `blocks_dangerous_namespaces`,
// etc.) only check a finite enumeration of method names. Verus
// upgrades these to "for ALL strings starting with `miner_`,
// `personal_`, `debug_`, `admin_`, the allowlist rejects them" —
// including method names geth hasn't shipped yet.
//
// The real `is_allowed_method` in chain_rpc_api.rs is trivially
// (`m.starts_with("eth_") || …`) — there's nothing to prove about
// the executable wrapper; it manifestly implements the spec. The
// content of this file is the SPEC + the security-relevant lemmas
// downstream of it.

#![allow(unused_imports)]

use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Spec layer: the four allowed JSON-RPC namespace prefixes, as
// ghost sequences of characters, and what "allowed" means.
// ---------------------------------------------------------------------------

pub open spec fn eth_prefix() -> Seq<char>     { seq!['e','t','h','_'] }
pub open spec fn net_prefix() -> Seq<char>     { seq!['n','e','t','_'] }
pub open spec fn web3_prefix() -> Seq<char>    { seq!['w','e','b','3','_'] }
pub open spec fn txpool_prefix() -> Seq<char>  { seq!['t','x','p','o','o','l','_'] }

/// `s` starts with `p` — standard subrange formulation.
pub open spec fn has_prefix(s: Seq<char>, p: Seq<char>) -> bool {
    p.len() <= s.len() && s.subrange(0, p.len() as int) == p
}

/// The allowlist contract: a method name is allowed iff it starts
/// with exactly one of the four read-only namespace prefixes. This
/// mirrors the runtime `ALLOWED_METHOD_PREFIXES.iter().any(...)`
/// check in chain_rpc_api.rs.
pub open spec fn spec_is_allowed(method: Seq<char>) -> bool {
    has_prefix(method, eth_prefix())
        || has_prefix(method, net_prefix())
        || has_prefix(method, web3_prefix())
        || has_prefix(method, txpool_prefix())
}

// ---------------------------------------------------------------------------
// Sanity: the prefixes are pairwise distinct in their first character
// (so an attacker can't smuggle a dangerous method by exploiting some
// shared prefix). Verus discharges these as constant-comparison
// trivialities.
// ---------------------------------------------------------------------------

pub proof fn lemma_allowed_first_chars_known()
    ensures
        eth_prefix()[0] == 'e',
        net_prefix()[0] == 'n',
        web3_prefix()[0] == 'w',
        txpool_prefix()[0] == 't',
{
}

// ---------------------------------------------------------------------------
// The four dangerous geth namespaces and their distinctness from the
// allowed prefixes.
// ---------------------------------------------------------------------------

pub open spec fn miner_prefix() -> Seq<char>     { seq!['m','i','n','e','r','_'] }
pub open spec fn personal_prefix() -> Seq<char>  { seq!['p','e','r','s','o','n','a','l','_'] }
pub open spec fn debug_prefix() -> Seq<char>     { seq!['d','e','b','u','g','_'] }
pub open spec fn admin_prefix() -> Seq<char>     { seq!['a','d','m','i','n','_'] }

pub proof fn lemma_dangerous_first_chars()
    ensures
        miner_prefix()[0] == 'm',
        personal_prefix()[0] == 'p',
        debug_prefix()[0] == 'd',
        admin_prefix()[0] == 'a',
{
}

// ---------------------------------------------------------------------------
// Core lemma: if `s` starts with a dangerous prefix whose first
// character isn't shared by ANY allowed prefix, then no allowed
// prefix is a prefix of `s`, so the spec rejects it.
//
// This is the proof-engineering pivot: by lifting the question "does
// every dangerous method name get rejected?" to "does `s[0]` clash
// with every allowed-prefix first character?", we turn an unbounded
// quantifier over method names into a finite check on 4 characters.
// ---------------------------------------------------------------------------

pub proof fn dangerous_namespace_is_rejected(s: Seq<char>, d: Seq<char>)
    requires
        d.len() >= 1,
        has_prefix(s, d),
        d[0] != eth_prefix()[0],
        d[0] != net_prefix()[0],
        d[0] != web3_prefix()[0],
        d[0] != txpool_prefix()[0],
    ensures
        !spec_is_allowed(s),
{
    // s starts with d (length >= 1), so s[0] == d[0].
    assert(s[0] == d[0]);
    // Each allowed prefix is non-empty and starts with a character
    // that differs from d[0], so if s starts with one of them then
    // s[0] would equal both d[0] and that prefix's first char —
    // contradiction.
    assert(eth_prefix().len() >= 1);
    assert(net_prefix().len() >= 1);
    assert(web3_prefix().len() >= 1);
    assert(txpool_prefix().len() >= 1);
    // Verus's subrange-equality lemma: if s.subrange(0, k) == p then
    // s[0] == p[0]. The four `has_prefix(s, …)` cases that would
    // make spec_is_allowed true are all refuted by s[0] != p[0].
}

// ---------------------------------------------------------------------------
// Per-namespace conclusions — these are the statements an auditor
// cares about. Each is a one-line application of the core lemma.
// ---------------------------------------------------------------------------

pub proof fn miner_methods_rejected(s: Seq<char>)
    requires has_prefix(s, miner_prefix())
    ensures !spec_is_allowed(s)
{
    dangerous_namespace_is_rejected(s, miner_prefix());
}

pub proof fn personal_methods_rejected(s: Seq<char>)
    requires has_prefix(s, personal_prefix())
    ensures !spec_is_allowed(s)
{
    dangerous_namespace_is_rejected(s, personal_prefix());
}

pub proof fn debug_methods_rejected(s: Seq<char>)
    requires has_prefix(s, debug_prefix())
    ensures !spec_is_allowed(s)
{
    dangerous_namespace_is_rejected(s, debug_prefix());
}

pub proof fn admin_methods_rejected(s: Seq<char>)
    requires has_prefix(s, admin_prefix())
    ensures !spec_is_allowed(s)
{
    dangerous_namespace_is_rejected(s, admin_prefix());
}

// ---------------------------------------------------------------------------
// Concrete sanity: a specific known-bad method gets rejected. This
// is what the example-based test `blocks_dangerous_namespaces` in
// chain_rpc_api.rs covers — included here so the Verus build doubles
// as a test that the proofs actually apply to real method names.
// ---------------------------------------------------------------------------

pub proof fn example_miner_set_etherbase_rejected()
    ensures !spec_is_allowed(seq!['m','i','n','e','r','_','s','e','t','E','t','h','e','r','b','a','s','e']),
{
    let s = seq!['m','i','n','e','r','_','s','e','t','E','t','h','e','r','b','a','s','e'];
    // s starts with miner_prefix (the first 6 chars match by
    // construction).
    assert(has_prefix(s, miner_prefix())) by {
        assert(s.subrange(0, 6int) == miner_prefix());
    }
    miner_methods_rejected(s);
}

} // verus!

fn main() {}
