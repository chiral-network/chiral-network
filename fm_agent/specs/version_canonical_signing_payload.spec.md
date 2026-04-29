// [SPEC]
// Unit: src-tauri/src/version.rs
//
// canonical_signing_payload(p: &VersionPolicy) -> Vec<u8>
//
// Pre-condition:
//   - p is a fully constructed VersionPolicy. No field is null. Fields may
//     hold any byte sequence representable by their declared types
//     (UTF-8 strings, including the NUL byte 0x00, for the string fields).
//
// Post-condition:
//   - Returns a byte sequence that is a one-to-one (injective) function
//     of (p.min_required, p.recommended, p.download_url, p.message,
//     p.issued_at, p.valid_until). Specifically: any two VersionPolicy
//     values that differ in at least one of those six fields MUST produce
//     two distinct return values.
//   - The function is deterministic: two calls with structurally equal
//     inputs return byte-equal outputs.
//   - The mapping is implementation-language independent: any other
//     implementation that follows the same documented schema must
//     produce the same bytes for the same input.
//   - The signature field is NOT included in the payload.
// [SPEC]

// [INFO]
// (no callees)
// [INFO]
