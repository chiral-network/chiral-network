// Probe: demonstrates that canonical_signing_payload (src-tauri/src/version.rs)
// is not an injective function of its inputs. Two distinct VersionPolicy
// values produce byte-identical signing payloads, which means a single
// Ed25519 signature would verify against both.
//
// Build & run from the repo root:
//   rustc --edition 2021 fm_agent/bug_validation/_probe_BUG-002-canonical-payload-nul-collision.rs -o /tmp/probe002
//   /tmp/probe002

#[derive(Clone)]
struct VersionPolicy {
    min_required: String,
    recommended: String,
    download_url: String,
    message: Option<String>,
    issued_at: u64,
    valid_until: u64,
}

// Verbatim copy of the production canonical_signing_payload — the unit
// under test. Any change here would invalidate the probe.
fn canonical_signing_payload(p: &VersionPolicy) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    let issued = p.issued_at.to_string();
    let valid = p.valid_until.to_string();
    let parts: [&[u8]; 6] = [
        p.min_required.as_bytes(),
        p.recommended.as_bytes(),
        p.download_url.as_bytes(),
        p.message.as_deref().unwrap_or("").as_bytes(),
        issued.as_bytes(),
        valid.as_bytes(),
    ];
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(0);
        }
        out.extend_from_slice(part);
    }
    out
}

fn main() {
    // Two policies that differ in two fields (min_required, recommended)
    // but produce byte-identical canonical payloads. Policy A embeds a
    // raw NUL inside min_required; Policy B places the bytes after the
    // NUL into recommended instead. Because the canonicaliser uses the
    // same NUL byte both as field separator and as a permitted field
    // content, both layouts emit the same bytes.
    let a = VersionPolicy {
        min_required: "1.0.0\0relaxed".to_string(),
        recommended: "9.9.9".to_string(),
        download_url: String::new(),
        message: None,
        issued_at: 1700000000,
        valid_until: 0,
    };
    let b = VersionPolicy {
        min_required: "1.0.0".to_string(),
        recommended: "relaxed\09.9.9".to_string(),
        download_url: String::new(),
        message: None,
        issued_at: 1700000000,
        valid_until: 0,
    };

    let pa = canonical_signing_payload(&a);
    let pb = canonical_signing_payload(&b);

    let policies_differ =
        a.min_required != b.min_required || a.recommended != b.recommended;

    if pa == pb && policies_differ {
        println!(
            "CONFIRMED — two distinct VersionPolicy values produce \
             byte-equal canonical payloads ({} bytes). \
             A.min_required={:?} A.recommended={:?} \
             B.min_required={:?} B.recommended={:?}",
            pa.len(),
            a.min_required,
            a.recommended,
            b.min_required,
            b.recommended
        );
    } else {
        println!(
            "NOT CONFIRMED — payloads differ (A={} bytes, B={} bytes) or policies are equal",
            pa.len(),
            pb.len()
        );
    }
}
