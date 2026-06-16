use chiral_network::relay_share_proxy::validate_relay_share_origin_url;

#[test]
fn relay_share_origin_accepts_http_and_https_origins() {
    assert!(validate_relay_share_origin_url("http://example.com:9419/").is_ok());
    assert!(validate_relay_share_origin_url("https://chiral.network/path?file=demo").is_ok());
}

#[test]
fn relay_share_origin_rejects_unsupported_schemes() {
    for origin in [
        "ftp://example.com:9419/",
        "gopher://example.com:9419/",
        "file:///tmp/chiral-origin",
        "example.com:9419",
        "://example.com:9419",
    ] {
        let err = validate_relay_share_origin_url(origin)
            .expect_err("unsupported scheme should be rejected");
        assert!(
            err.contains("scheme must be http:// or https://"),
            "origin {origin} returned unexpected error: {err}"
        );
    }
}

#[test]
fn relay_share_origin_rejects_malformed_http_origins() {
    for origin in ["http://", "https://", "http://?token=missing-host"] {
        let err =
            validate_relay_share_origin_url(origin).expect_err("missing host should be rejected");
        assert!(
            err.contains("origin_url has no host"),
            "origin {origin} returned unexpected error: {err}"
        );
    }
}
