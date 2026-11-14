//! Commercial declaration contracts for specify-inference-commercial-terms task 2.1.
//! Synthetic metadata only: no measured qualification, live offers, clocks, keys or I/O.
use chiral_network::{
    offer_registry::validate_model,
    protocol::{InferenceOffer, ModelSpec, QualificationIdentity},
};
use serde_json::{json, Value};

fn model_document() -> Value {
    json!({
        "id": "test/model",
        "revision": "ab".repeat(20),
        "tokenizer_revision": "cd".repeat(20),
        "chat_template_sha256": "ef".repeat(32),
        "quantization": null,
        "context_length": 4096,
        "max_output_tokens": 256,
        "capabilities": ["chat"],
        "serving": null
    })
}

fn profiled_model_document() -> Value {
    let mut model = model_document();
    model["serving"] = json!({
        "hf_repo": "test/model",
        "tokenizer_repo": "test/tokenizer",
        "artifact_manifest_sha256": "a".repeat(64),
        "engine": "llama.cpp",
        "engine_version": "test-only",
        "runtime_sha256": "b".repeat(64),
        "platform": "linux",
        "backend": "cuda",
        "memory_kind": "dedicated",
        "devices": [{
            "id": "synthetic-device-0",
            "name": "synthetic device, not measured hardware",
            "memory_mib": 32000,
            "driver_version": "test-only",
            "engine_device": "CUDA0"
        }],
        "memory_reserve_mib": 256,
        "gpu_layers": 999,
        "split_mode": "none",
        "tensor_split": [],
        "cache_type_k": "f16",
        "cache_type_v": "f16",
        "flash_attention": true,
        "batch_size": 512,
        "ubatch_size": 128,
        "capacity_tier": "32gb",
        "region": "test",
        "max_concurrency": 2,
        "arrival_milli_rps": 1000,
        "output_tokens_per_second": 20,
        "policy": "warm-generation-v1",
        "qualification": {"evidence_sha256": "c".repeat(64), "measured_at": 100}
    });
    model
}

fn offer_document() -> Value {
    // A schema fixture, not a currently admissible offer. No signature or clock is involved.
    json!({
        "network": {
            "chain_id": 71,
            "genesis_hash": "0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804",
            "deployment_id": "commercial-terms-test"
        },
        "provider_wallet": "0x1111111111111111111111111111111111111111",
        // Base58 SHA-256 multihash with the fixed digest [0x11; 32].
        "peer_id": "QmPVGjYFugq4XUyBfoTHG6c3qxfBS26jEdaFM1gdAVuMZ2",
        "deployment_id": "provider-a",
        "endpoint": "https://inference.example.org",
        "model": profiled_model_document(),
        "price": {"input_wei_per_1k": "1", "output_wei_per_1k": "2"},
        "min_funding_wei": "1000",
        "platform_wallet": "0x2222222222222222222222222222222222222222",
        "fee_bps": 50,
        "max_concurrency": 2,
        "revision": 1,
        "issued_at": 100,
        "expires_at": 220
    })
}

fn replaced(document: &Value, pointer: &str, value: Value) -> Value {
    let mut changed = document.clone();
    *changed.pointer_mut(pointer).expect("known fixture field") = value;
    changed
}

fn assert_model_valid(document: Value, case: &str) {
    let model: ModelSpec = serde_json::from_value(document).expect("well-typed model fixture");
    assert_eq!(validate_model(&model), Ok(()), "{case}");
}

fn assert_model_invalid(document: Value, case: &str) {
    // A schema error must not accidentally satisfy a semantic rejection assertion.
    let model: ModelSpec = serde_json::from_value(document).expect("well-typed adversarial model");
    assert!(validate_model(&model).is_err(), "{case}");
}

fn with_devices(count: usize) -> Value {
    let mut model = profiled_model_document();
    let device = model["serving"]["devices"][0].clone();
    model["serving"]["devices"] = Value::Array(
        (0..count)
            .map(|index| {
                let mut next = device.clone();
                next["id"] = json!(format!("synthetic-device-{index}"));
                next["engine_device"] = json!(format!("CUDA{index}"));
                next
            })
            .collect(),
    );
    if count > 1 {
        model["serving"]["split_mode"] = json!("layer");
        model["serving"]["tensor_split"] = json!(vec![1; count]);
    }
    model
}

#[test]
fn commercial_schema_rejects_unknown_fields_at_every_new_object_boundary() {
    let base = offer_document();
    serde_json::from_value::<InferenceOffer>(base.clone()).unwrap();
    for path in [
        "",
        "/model",
        "/model/serving",
        "/model/serving/devices/0",
        "/model/serving/qualification",
    ] {
        let mut changed = base.clone();
        changed
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unrecognized_term".into(), json!("not signed typed metadata"));
        assert!(
            serde_json::from_value::<InferenceOffer>(changed).is_err(),
            "unknown commercial field at {path:?} must not be silently discarded"
        );
    }
}

#[test]
fn commercial_schema_requires_all_nonoptional_terms() {
    let base = offer_document();
    serde_json::from_value::<InferenceOffer>(base.clone()).unwrap();
    let required: &[(&str, &[&str])] = &[
        (
            "",
            &[
                "network", "provider_wallet", "peer_id", "deployment_id", "endpoint", "model",
                "price", "min_funding_wei", "platform_wallet", "fee_bps", "max_concurrency",
                "revision", "issued_at", "expires_at",
            ],
        ),
        (
            "/model",
            &[
                "id", "revision", "tokenizer_revision", "chat_template_sha256", "context_length",
                "max_output_tokens", "capabilities",
            ],
        ),
        (
            "/model/serving",
            &[
                "hf_repo", "tokenizer_repo", "artifact_manifest_sha256", "engine", "engine_version",
                "runtime_sha256", "platform", "backend", "memory_kind", "devices",
                "memory_reserve_mib", "gpu_layers", "split_mode", "tensor_split", "cache_type_k",
                "cache_type_v", "flash_attention", "batch_size", "ubatch_size", "capacity_tier",
                "region", "max_concurrency", "arrival_milli_rps", "output_tokens_per_second",
                "policy", "qualification",
            ],
        ),
        (
            "/model/serving/devices/0",
            &["id", "name", "memory_mib", "driver_version", "engine_device"],
        ),
        (
            "/model/serving/qualification",
            &["evidence_sha256", "measured_at"],
        ),
    ];
    for (path, fields) in required {
        for field in *fields {
            let mut changed = base.clone();
            assert!(changed
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(*field)
                .is_some());
            assert!(
                serde_json::from_value::<InferenceOffer>(changed).is_err(),
                "missing {path}/{field} must not acquire implicit terms"
            );
        }
    }
}

#[test]
fn commercial_schema_rejects_wrong_scalar_and_container_types() {
    let base = offer_document();
    serde_json::from_value::<InferenceOffer>(base.clone()).unwrap();
    for (path, value) in [
        ("/provider_wallet", json!(1)),
        ("/peer_id", json!(null)),
        ("/endpoint", json!(false)),
        ("/min_funding_wei", json!(1000)),
        ("/fee_bps", json!(65536)),
        ("/max_concurrency", json!(4294967296_u64)),
        ("/revision", json!(-1)),
        ("/issued_at", json!("100")),
        ("/expires_at", json!(220.5)),
        ("/model/quantization", json!(4)),
        ("/model/capabilities", json!("chat")),
        ("/model/capabilities/0", json!(1)),
        ("/model/serving", json!([])),
        ("/model/serving/devices", json!({})),
        ("/model/serving/devices/0/id", json!(false)),
        ("/model/serving/flash_attention", json!(1)),
        ("/model/serving/qualification", json!(null)),
        ("/model/serving/tensor_split", json!(["1"])),
    ] {
        assert!(
            serde_json::from_value::<InferenceOffer>(replaced(&base, path, value)).is_err(),
            "wrong wire type at {path}"
        );
    }
}

#[test]
fn model_and_profile_integer_fields_preserve_unsigned_widths() {
    let base = profiled_model_document();
    assert_model_valid(base.clone(), "baseline profile");
    let u32_fields = [
        "/serving/gpu_layers",
        "/serving/batch_size",
        "/serving/ubatch_size",
        "/serving/max_concurrency",
        "/serving/arrival_milli_rps",
        "/serving/output_tokens_per_second",
    ];
    let u64_fields = [
        "/context_length",
        "/max_output_tokens",
        "/serving/memory_reserve_mib",
        "/serving/devices/0/memory_mib",
        "/serving/qualification/measured_at",
    ];
    for path in u32_fields.iter().chain(u64_fields.iter()) {
        for invalid in [json!(-1), json!(1.5), json!("1"), Value::Null] {
            assert!(
                serde_json::from_value::<ModelSpec>(replaced(&base, path, invalid)).is_err(),
                "unsigned integer at {path} must not be coerced"
            );
        }
    }
    for path in u32_fields {
        assert!(
            serde_json::from_value::<ModelSpec>(replaced(&base, path, json!(4294967296_u64)))
                .is_err(),
            "u32 overflow at {path}"
        );
    }
    for invalid in [json!([-1]), json!([1.5]), json!([4294967296_u64])] {
        assert!(serde_json::from_value::<ModelSpec>(replaced(
            &base,
            "/serving/tensor_split",
            invalid
        ))
        .is_err());
    }
    // Exact wire width, independent of the separate current-time offer admission check.
    let largest = format!(
        r#"{{"evidence_sha256":"{}","measured_at":18446744073709551615}}"#,
        "c".repeat(64)
    );
    let identity: QualificationIdentity = serde_json::from_str(&largest).unwrap();
    assert_eq!(identity.measured_at, u64::MAX);
    let overflow = largest.replace("18446744073709551615", "18446744073709551616");
    assert!(serde_json::from_str::<QualificationIdentity>(&overflow).is_err());
}

#[test]
fn nullable_model_metadata_does_not_invent_execution_claims() {
    let explicit_null = model_document();
    let mut omitted = explicit_null.clone();
    omitted.as_object_mut().unwrap().remove("quantization");
    omitted.as_object_mut().unwrap().remove("serving");
    let model: ModelSpec = serde_json::from_value(omitted).unwrap();
    assert!(model.quantization.is_none());
    assert!(model.serving.is_none());
    // Omitted options normalize to explicit null, not new signed model/profile terms.
    assert_eq!(serde_json::to_value(&model).unwrap(), explicit_null);
    assert_eq!(validate_model(&model), Ok(()));
}

#[test]
fn model_identifiers_are_bounded_in_utf8_bytes_and_exclude_controls() {
    let base = model_document();
    assert_model_valid(replaced(&base, "/id", json!("é".repeat(128))), "256 UTF-8 bytes");
    for invalid in ["".into(), "é".repeat(129), "model\n".into(), "model\u{0085}".into()] {
        assert_model_invalid(replaced(&base, "/id", json!(invalid)), "invalid model ID");
    }
}

#[test]
fn immutable_model_revisions_and_template_digests_have_exact_hex_lengths() {
    let base = model_document();
    for path in ["/revision", "/tokenizer_revision"] {
        for valid in ["ab".repeat(20), "CD".repeat(32)] {
            assert_model_valid(replaced(&base, path, json!(valid)), path);
        }
        for invalid in [
            "main".into(), "g".repeat(40), "a".repeat(39), "a".repeat(41),
            "a".repeat(63), "a".repeat(65),
        ] {
            assert_model_invalid(replaced(&base, path, json!(invalid)), path);
        }
    }
    assert_model_valid(
        replaced(&base, "/chat_template_sha256", json!("AB".repeat(32))),
        "model template hex is not restricted to lowercase",
    );
    for invalid in ["a".repeat(63), "a".repeat(65), "z".repeat(64)] {
        assert_model_invalid(
            replaced(&base, "/chat_template_sha256", json!(invalid)),
            "invalid template digest",
        );
    }
}

#[test]
fn quantization_labels_are_nullable_but_nonempty_and_byte_bounded_when_present() {
    let base = model_document();
    assert_model_valid(base.clone(), "unquantized model");
    assert_model_valid(
        replaced(&base, "/quantization", json!("é".repeat(32))),
        "64-byte quantization label",
    );
    for invalid in [String::new(), "é".repeat(32) + "x"] {
        assert_model_invalid(replaced(&base, "/quantization", json!(invalid)), "invalid label");
    }
}

#[test]
fn unprofiled_generation_limits_keep_the_exact_context_and_output_bounds() {
    let base = model_document();
    for limit in [1, 1_048_576] {
        let mut model = base.clone();
        model["context_length"] = json!(limit);
        model["max_output_tokens"] = json!(limit);
        assert_model_valid(model, "inclusive context/output boundary");
    }
    for (context, output) in [(0, 0), (1_048_577, 1), (4096, 4097), (4096, 0)] {
        let mut model = base.clone();
        model["context_length"] = json!(context);
        model["max_output_tokens"] = json!(output);
        assert_model_invalid(model, "invalid generation limits");
    }
    let mut completions = base;
    completions["capabilities"] = json!(["completions"]);
    completions["max_output_tokens"] = json!(0);
    assert_model_invalid(completions, "completions is also a generating endpoint");
}

#[test]
fn capabilities_are_unique_known_endpoints_not_just_auxiliary_features() {
    let base = model_document();
    for invalid in [
        json!([]), json!(["chat", "chat"]), json!(["chat", "video"]),
        json!(["tools"]), json!(["structured_output", "streaming"]),
    ] {
        assert_model_invalid(replaced(&base, "/capabilities", invalid), "invalid capability set");
    }
    // Broad declaration syntax is not a native task/qualification certificate.
    assert_model_valid(
        replaced(
            &base,
            "/capabilities",
            json!(["chat", "completions", "embeddings", "tools", "structured_output", "streaming"]),
        ),
        "all six distinct declaration capabilities",
    );
    let mut embeddings = base;
    embeddings["capabilities"] = json!(["embeddings", "streaming"]);
    embeddings["max_output_tokens"] = json!(0);
    assert_model_valid(embeddings.clone(), "auxiliary streaming does not invent generated tokens");
    embeddings["max_output_tokens"] = json!(1);
    assert_model_invalid(embeddings, "nongenerating endpoint still has zero output budget");
}

#[test]
fn profile_text_fields_have_byte_limits_and_no_control_characters() {
    let base = profiled_model_document();
    for path in [
        "/serving/hf_repo", "/serving/tokenizer_repo", "/serving/engine_version", "/serving/region",
        "/serving/devices/0/id", "/serving/devices/0/name", "/serving/devices/0/driver_version",
        "/serving/devices/0/engine_device",
    ] {
        assert_model_valid(replaced(&base, path, json!("é".repeat(128))), path);
        for invalid in [String::new(), "é".repeat(129), "text\u{0085}".into()] {
            assert_model_invalid(replaced(&base, path, json!(invalid)), path);
        }
    }
}

#[test]
fn profile_digests_are_lowercase_even_when_model_revision_hex_is_not() {
    let base = profiled_model_document();
    for path in [
        "/serving/artifact_manifest_sha256",
        "/serving/runtime_sha256",
        "/serving/qualification/evidence_sha256",
    ] {
        assert_model_valid(replaced(&base, path, json!("0123456789abcdef".repeat(4))), path);
        for invalid in ["A".repeat(64), "g".repeat(64), "a".repeat(63), "a".repeat(65)] {
            assert_model_invalid(replaced(&base, path, json!(invalid)), path);
        }
    }
}

#[test]
fn declared_platform_backend_and_memory_kind_must_be_compatible() {
    let base = profiled_model_document();
    for (platform, backend, memory, valid) in [
        ("linux", "cuda", "dedicated", true),
        ("windows", "cuda", "dedicated", true),
        ("linux", "vulkan", "dedicated", true),
        ("windows", "vulkan", "dedicated", true),
        ("linux", "vulkan", "unified", true),
        ("macos", "metal", "unified", true),
        ("macos", "cuda", "dedicated", false),
        ("macos", "vulkan", "unified", false),
        ("linux", "metal", "unified", false),
        ("windows", "metal", "unified", false),
        ("linux", "cpu", "dedicated", false),
        ("linux", "cuda", "unified", false),
        ("macos", "metal", "dedicated", false),
        ("linux", "cuda", "shared", false),
        ("unknown", "cuda", "dedicated", false),
    ] {
        let mut model = base.clone();
        model["serving"]["platform"] = json!(platform);
        model["serving"]["backend"] = json!(backend);
        model["serving"]["memory_kind"] = json!(memory);
        model["serving"]["memory_reserve_mib"] = json!(4096);
        model["serving"]["devices"][0]["engine_device"] = json!(match backend {
            "metal" => "MTL0",
            "vulkan" => "Vulkan0",
            _ => "CUDA0",
        });
        let case = format!("{platform}/{backend}/{memory}");
        if valid {
            assert_model_valid(model, &case);
        } else {
            assert_model_invalid(model, &case);
        }
    }
}

#[test]
fn device_inventory_is_bounded_and_cannot_double_count_physical_or_engine_ids() {
    assert_model_valid(with_devices(64), "maximum declared device count, not native qualification");
    assert_model_invalid(with_devices(0), "empty device inventory");
    assert_model_invalid(with_devices(65), "too many declared devices");
    let base = with_devices(2);
    assert_model_valid(base.clone(), "two independently identified devices");
    for field in ["id", "engine_device"] {
        let mut duplicate = base.clone();
        duplicate["serving"]["devices"][1][field] = base["serving"]["devices"][0][field].clone();
        assert_model_invalid(duplicate, field);
    }
}

#[test]
fn larger_capacity_tiers_keep_per_device_minima_except_the_explicit_sum_tier() {
    for (tier, minimum) in [("16gb", 15000), ("32gb", 30000)] {
        let mut model = with_devices(2);
        model["serving"]["capacity_tier"] = json!(tier);
        model["serving"]["devices"][0]["memory_mib"] = json!(minimum);
        model["serving"]["devices"][1]["memory_mib"] = json!(minimum * 2);
        assert_model_valid(model.clone(), tier);
        model["serving"]["devices"][0]["memory_mib"] = json!(minimum - 1);
        assert_model_invalid(model, "a larger second device cannot fill a per-device shortfall");
    }
    let mut higher = with_devices(2);
    higher["serving"]["capacity_tier"] = json!("higher");
    for device in higher["serving"]["devices"].as_array_mut().unwrap() {
        device["memory_mib"] = json!(22500);
    }
    assert_model_valid(higher.clone(), "45,000 MiB aggregate boundary");
    higher["serving"]["devices"][0]["memory_mib"] = json!(22499);
    assert_model_invalid(higher, "aggregate one MiB below higher tier");
    assert_model_invalid(
        replaced(&profiled_model_document(), "/serving/capacity_tier", json!("64gb")),
        "unsupported tier must not be inferred from its name",
    );
}

#[test]
fn dedicated_memory_reserve_and_physical_inventory_bounds_are_strict() {
    let base = profiled_model_document();
    assert_model_valid(base.clone(), "256 MiB dedicated reserve");
    assert_model_invalid(replaced(&base, "/serving/memory_reserve_mib", json!(255)), "reserve floor");
    assert_model_valid(
        replaced(&base, "/serving/memory_reserve_mib", json!(31999)),
        "capacity uses physical memory; reserve must still leave positive memory",
    );
    assert_model_invalid(
        replaced(&base, "/serving/memory_reserve_mib", json!(32000)),
        "reserve consumes the entire physical device",
    );
    assert_model_valid(
        replaced(&base, "/serving/devices/0/memory_mib", json!(1_u64 << 30)),
        "maximum allowed physical-memory declaration, not evidence of real capacity",
    );
    assert_model_invalid(
        replaced(&base, "/serving/devices/0/memory_mib", json!((1_u64 << 30) + 1)),
        "physical memory declaration exceeds its bound",
    );
}

#[test]
fn tensor_split_binds_exactly_the_selected_devices_with_bounded_positive_weights() {
    let single = profiled_model_document();
    assert_model_invalid(replaced(&single, "/serving/split_mode", json!("layer")), "single-device split");
    assert_model_invalid(replaced(&single, "/serving/tensor_split", json!([1])), "single-device weights");
    let multiple = with_devices(2);
    for mode in ["layer", "row"] {
        let mut model = multiple.clone();
        model["serving"]["split_mode"] = json!(mode);
        model["serving"]["tensor_split"] = json!([1, 1_000_000]);
        assert_model_valid(model, "supported multi-device split and inclusive weight bounds");
    }
    for mode in ["none", "auto"] {
        assert_model_invalid(replaced(&multiple, "/serving/split_mode", json!(mode)), "invalid split");
    }
    for weights in [json!([]), json!([1]), json!([1, 1, 1]), json!([0, 1]), json!([1, 1_000_001])] {
        assert_model_invalid(replaced(&multiple, "/serving/tensor_split", weights), "invalid weights");
    }
}

#[test]
fn cache_modes_and_flash_attention_are_validated_as_one_execution_contract() {
    let base = profiled_model_document();
    for cache in ["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"] {
        let mut model = base.clone();
        model["serving"]["cache_type_k"] = json!(cache);
        model["serving"]["cache_type_v"] = json!(cache);
        assert_model_valid(model.clone(), "supported caches with flash attention");
        model["serving"]["flash_attention"] = json!(false);
        if ["f32", "f16", "bf16"].contains(&cache) {
            assert_model_valid(model, "unquantized value cache without flash attention");
        } else {
            assert_model_invalid(model, "quantized value cache requires flash attention");
        }
    }
    for field in ["cache_type_k", "cache_type_v"] {
        assert_model_invalid(
            replaced(&base, &format!("/serving/{field}"), json!("q2_k")),
            "unsupported cache type",
        );
    }
    let mut quantized_key = base;
    quantized_key["serving"]["cache_type_k"] = json!("q4_0");
    quantized_key["serving"]["flash_attention"] = json!(false);
    assert_model_valid(quantized_key, "the flash-attention constraint is on the value cache");
}

#[test]
fn profile_execution_and_admission_numbers_keep_their_exact_bounds() {
    let base = profiled_model_document();
    for (field, low, high) in [
        ("gpu_layers", 1, 999),
        ("max_concurrency", 1, 256),
        ("arrival_milli_rps", 1, 1_000_000),
    ] {
        let path = format!("/serving/{field}");
        for valid in [low, high] {
            assert_model_valid(replaced(&base, &path, json!(valid)), field);
        }
        for invalid in [low - 1, high + 1] {
            assert_model_invalid(replaced(&base, &path, json!(invalid)), field);
        }
    }
    for batch in [1, 8192] {
        let mut model = base.clone();
        model["serving"]["batch_size"] = json!(batch);
        model["serving"]["ubatch_size"] = json!(batch);
        assert_model_valid(model, "inclusive batch boundary and equal microbatch");
    }
    for (batch, ubatch) in [(0, 1), (8193, 1), (512, 0), (512, 513)] {
        let mut model = base.clone();
        model["serving"]["batch_size"] = json!(batch);
        model["serving"]["ubatch_size"] = json!(ubatch);
        assert_model_invalid(model, "invalid batch/microbatch contract");
    }
    assert_model_invalid(
        replaced(&base, "/serving/qualification/measured_at", json!(0)),
        "qualification measurement time must be positive",
    );
}

#[test]
fn unsupported_engine_policy_and_speed_classes_cannot_be_inferred() {
    let base = profiled_model_document();
    for speed in [20, 50, 100] {
        assert_model_valid(
            replaced(&base, "/serving/output_tokens_per_second", json!(speed)),
            "supported declared class, not proof of performance",
        );
    }
    for speed in [0, 19, 21, 49, 51, 99, 101] {
        assert_model_invalid(
            replaced(&base, "/serving/output_tokens_per_second", json!(speed)),
            "unsupported speed class",
        );
    }
    assert_model_invalid(replaced(&base, "/serving/engine", json!("vllm")), "unsupported engine");
    assert_model_invalid(
        replaced(&base, "/serving/policy", json!("warm-generation-v2")),
        "unsupported performance policy",
    );
}
