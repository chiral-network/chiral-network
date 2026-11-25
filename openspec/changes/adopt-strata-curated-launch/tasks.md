## Implementation

- [x] 1. Revise the book and repository guidance for Strata-first curated launch; preserve legacy evidence and independent milestones.
- [x] 2. Pin the two catalog configurations, exact upstream source, artifact hashes, limits and capabilities.
- [x] 3. Implement explicit artifact installation and complete runtime packaging/inventory verification without automatic inference startup.
- [x] 4. Add Rust selection, immutable-profile checks, private interpreter/native process ownership and observed backend/context validation.
- [x] 5. Add exact prompt preparation, bounded text-chat bridge and accepted-token terminal accounting; reject unsupported requests before reservation.
- [x] 6. Add STOP/drain cancellation, native/bridge exit handling and no-retry/no-fallback behavior.
- [x] 7. Adapt provider generation and qualification tooling to Strata identities and host-RAM headroom.
- [x] 8. Reconcile affected active OpenSpec scope and write the operator guide.

## Verification

- [x] 9. Test curated contracts, inventory tampering, unsupported parameters and private token normalization in Rust/Python.
- [x] 10. Test native protocol usage mismatch, draft exclusion, cancellation, busy admission, process death and authenticated HTTP behavior.
- [x] 11. Build the exact native source and run CPU/Metal self-checks; verify exact tokenizer/template files and multilingual/control-token handling.
- [x] 12. Complete repository regression checks and strict OpenSpec validation; record results.

Validation: 510 Rust library tests passed (12 process helpers intentionally ignored); final affected Strata and process-tree tests passed after review fixes. Headless all-target compile check passed. All 92 Python tests passed, including the optional pinned-tokenizer integration. Frontend: 124 tests, type check and production build passed. Strict OpenSpec validation: 35 items passed.

## Real-model and release gates

- [ ] 13. Run SQ-10 and SQ-15 through a complete packaged runtime and actual provider/gateway/stock SDK, including cancellation and failure accounting.
- [ ] 14. Measure each offered CUDA/Metal platform and hardware profile, including sustained host RAM/VRAM headroom and exact speed policy. Do not reuse legacy evidence.
- [ ] 15. Qualify native platform installers and complete the existing public release/security gates before authorized rollout.

Tasks 13–15 are not satisfied by protocol fixtures or native self-checks. Local disk space is smaller than the smallest weight file, and the recorded zoo-6 SSH alias is unavailable on this workstation. Entries stay `unqualified`; public deployment remains unauthorized. Separate desktop/local-client/libp2p changes are not counted as work completed by this change.
