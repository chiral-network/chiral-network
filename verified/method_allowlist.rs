// Specification companion to chain_rpc_api::allowed_method.
// Exact method membership, never namespace membership. Checked with Verus,
// separately from Cargo; this file does not claim the runtime was verified.
use vstd::prelude::*;
verus! {
pub open spec fn allowed_methods() -> Set<Seq<char>> {
    set![
        "eth_chainId"@,
        "eth_blockNumber"@,
        "eth_gasPrice"@,
        "eth_getBalance"@,
        "eth_getTransactionCount"@,
        "eth_getTransactionReceipt"@,
        "eth_sendRawTransaction"@,
        "eth_syncing"@,
        "eth_getBlockByNumber"@,
        "eth_estimateGas"@
    ]
}
pub open spec fn spec_is_allowed(method: Seq<char>) -> bool {
    allowed_methods().contains(method)
}
pub proof fn unlisted_methods_are_rejected(method: Seq<char>)
    requires !allowed_methods().contains(method),
    ensures !spec_is_allowed(method),
{}
pub proof fn operator_methods_are_rejected()
    ensures
        !spec_is_allowed("eth_submitWork"@),
        !spec_is_allowed("eth_submitHashrate"@),
        !spec_is_allowed("debug_traceTransaction"@),
        !spec_is_allowed("personal_sign"@),
        !spec_is_allowed("admin_stopRPC"@),
        !spec_is_allowed("eth_sendTransaction"@),
{}
}
fn main() {}
