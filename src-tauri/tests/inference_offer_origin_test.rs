//! Advertised-origin contracts for specify-inference-commercial-terms task 2.1.
//! These exercise syntax/address admission only: no DNS, sockets, clocks or reachability claims.
use chiral_network::offer_registry::validate_endpoint;

#[test]
fn public_https_origins_accept_default_port_and_standard_url_normalization() {
    for (input, expected) in [
        (
            "https://inference.example.org",
            "https://inference.example.org/",
        ),
        (
            "HTTPS://Inference.Example.Org:443/",
            "https://inference.example.org/",
        ),
        ("https://1.1.1.1:443", "https://1.1.1.1/"),
        (
            "https://[2606:4700:4700::1111]:443/",
            "https://[2606:4700:4700::1111]/",
        ),
    ] {
        for development in [false, true] {
            let origin = validate_endpoint(input, development).unwrap();
            assert_eq!(origin.as_str(), expected);
            assert_eq!(origin.port_or_known_default(), Some(443));
        }
    }
}

#[test]
fn development_does_not_relax_remote_scheme_or_port_requirements() {
    for input in [
        "http://inference.example.org",
        "http://1.1.1.1:443",
        "https://inference.example.org:80",
        "https://inference.example.org:8443",
        "ws://inference.example.org",
        "wss://inference.example.org",
        "file:///tmp/model.gguf",
        "//inference.example.org",
    ] {
        for development in [false, true] {
            assert!(
                validate_endpoint(input, development).is_err(),
                "remote exception for {input}, development={development}"
            );
        }
    }
}

#[test]
fn origin_only_rules_also_apply_to_the_loopback_exception() {
    for (base, development) in [
        ("https://inference.example.org", false),
        ("https://inference.example.org", true),
        ("http://127.0.0.1:8080", true),
    ] {
        validate_endpoint(base, development).unwrap();
        for suffix in [
            "/v1",
            "/v1/",
            "//",
            "/%2f",
            "\\v1",
            "?",
            "?token=test-only",
            "#",
            "#fragment",
        ] {
            let input = format!("{base}{suffix}");
            assert!(
                validate_endpoint(&input, development).is_err(),
                "non-origin URL accepted: {input}, development={development}"
            );
        }
    }
}

#[test]
fn credentials_cannot_be_hidden_in_public_or_development_origins() {
    for (authority, development) in [
        ("inference.example.org", false),
        ("inference.example.org", true),
        ("127.0.0.1:8080", true),
    ] {
        validate_endpoint(&format!("https://{authority}"), development).unwrap();
        for userinfo in ["user@", "user:test-only@", ":test-only@", "%75ser@"] {
            let input = format!("https://{userinfo}{authority}/");
            assert!(
                validate_endpoint(&input, development).is_err(),
                "credential-bearing URL accepted: {input}, development={development}"
            );
        }
    }
}

#[test]
fn literal_loopback_and_exact_localhost_require_explicit_development() {
    for (input, expected) in [
        ("http://127.0.0.1:8080", "http://127.0.0.1:8080/"),
        ("https://127.0.0.2", "https://127.0.0.2/"),
        ("https://127.0.0.2:8443", "https://127.0.0.2:8443/"),
        ("http://[::1]:8080", "http://[::1]:8080/"),
        ("https://[::1]", "https://[::1]/"),
        ("HTTP://LOCALHOST:8080", "http://localhost:8080/"),
        ("https://localhost", "https://localhost/"),
    ] {
        assert!(
            validate_endpoint(input, false).is_err(),
            "production accepted local origin: {input}"
        );
        assert_eq!(validate_endpoint(input, true).unwrap().as_str(), expected);
    }
}

#[test]
fn development_never_extends_to_private_lan_or_localhost_subdomains() {
    for input in [
        "https://192.168.10.1",
        "https://172.16.0.1",
        "https://172.31.255.254",
        "https://[fc00::1]",
        "https://[fe80::1]",
        "https://provider.localhost",
        "https://PROVIDER.LOCALHOST",
    ] {
        for development in [false, true] {
            assert!(
                validate_endpoint(input, development).is_err(),
                "non-loopback development exception: {input}, development={development}"
            );
        }
    }
}

#[test]
fn reserved_ipv4_literals_cannot_be_advertised_as_public_origins() {
    for host in [
        "0.0.0.0",
        "0.1.2.3",
        "100.64.0.1",
        "100.127.255.254",
        "192.0.0.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.19.255.254",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.255",
        "240.0.0.1",
        "255.255.255.255",
    ] {
        let input = format!("https://{host}");
        for development in [false, true] {
            assert!(
                validate_endpoint(&input, development).is_err(),
                "reserved IPv4 origin accepted: {input}, development={development}"
            );
        }
    }
}

#[test]
fn nonpublic_ipv6_and_mapped_ipv4_destinations_are_rejected() {
    for host in [
        "::",
        "fdff::1",
        "ff02::1",
        "2001::1",
        "2001:100::1",
        "2001:db8::1",
        "2002:7f00:1::",
        "3fff::1",
        "3fff:fff::1",
        "::ffff:10.0.0.1",
        "::ffff:169.254.169.254",
        "::ffff:192.0.2.1",
    ] {
        let input = format!("https://[{host}]");
        for development in [false, true] {
            assert!(
                validate_endpoint(&input, development).is_err(),
                "nonpublic IPv6 origin accepted: {input}, development={development}"
            );
        }
    }
}

#[test]
fn legacy_ipv4_spellings_cannot_disguise_loopback_or_metadata_addresses() {
    for (host, address) in [
        ("127.1", "127.0.0.1"),
        ("2130706433", "127.0.0.1"),
        ("0x7f000001", "127.0.0.1"),
        ("0177.0.0.1", "127.0.0.1"),
        ("2852039166", "169.254.169.254"),
    ] {
        let input = format!("https://{host}");
        // Prove this is a recognized IP spelling, not merely an unparseable URL.
        assert_eq!(url::Url::parse(&input).unwrap().host_str(), Some(address));
        assert!(
            validate_endpoint(&input, false).is_err(),
            "normalized nonpublic address accepted: {input}"
        );
    }
}
