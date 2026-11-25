//! Curated Strata runtime boundary. Legacy llama.cpp is never an implicit fallback.
use super::*;
use serde_json::json;

pub const ENGINE_VERSION: &str = "0.2.22";
pub const SOURCE_REVISION: &str = "b28ca95e3e6847f2d5e317994f253b66716deeaa";
const CATALOG: &str = include_str!("../../dev/strata/catalog.json");
const UPSTREAM: &str = include_str!("../../dev/strata/upstream-files.json");

pub fn selected(model: &ModelConfig) -> bool {
    model.runtime.as_ref().is_some_and(|r| r.strata_catalog.is_some())
}

pub fn catalog_entry(id: &str) -> Result<Value, String> {
    let catalog: Value = serde_json::from_str(CATALOG).map_err(|_| "invalid compiled Strata catalog")?;
    catalog["entries"].as_array().ok_or("missing Strata catalog")?.iter()
        .find(|e| e["id"].as_str() == Some(id)).cloned().ok_or("model is outside curated Strata catalog".into())
}

fn entry_model(e: &Value) -> Result<ModelSpec, String> {
    serde_json::from_value(json!({
        "id":format!("{}/{}", e["repo_id"].as_str().ok_or("missing catalog repo")?, e["model_file"].as_str().ok_or("missing catalog file")?),
        "revision": e["revision"], "tokenizer_revision": e["revision"],
        "chat_template_sha256":e["files"]["chat_template.jinja"]["sha256"],
        "quantization":e["quantization"], "context_length":e["context_length"],
        "max_output_tokens":e["max_output_tokens"], "capabilities":e["capabilities"], "serving":null
    })).map_err(|_| "invalid catalog ModelSpec".into())
}

pub fn validate_profile(model: &ModelSpec, p: &crate::protocol::ServingProfile) -> Result<(), String> {
    let catalog: Value = serde_json::from_str(CATALOG).map_err(|_| "invalid Strata catalog")?;
    let mut base = model.clone(); base.serving = None;
    let e = catalog["entries"].as_array().ok_or("missing catalog")?.iter()
        .find(|e| entry_model(e).is_ok_and(|m| m == base)).ok_or("Strata profile is outside curated catalog")?;
    if p.engine_version != ENGINE_VERSION || p.hf_repo != e["repo_id"] || p.tokenizer_repo != e["repo_id"]
        || p.devices.len() != 1 || !matches!(p.backend.as_str(), "cuda" | "metal")
        || p.max_concurrency != 1 || p.gpu_layers != 0 || p.split_mode != "none" || !p.tensor_split.is_empty()
        || p.cache_type_k != "f16" || p.cache_type_v != "f16" || p.flash_attention
        || p.batch_size != 512 || p.ubatch_size != 512 {
        return Err("Strata serving profile differs from curated execution recipe".into());
    }
    Ok(())
}

pub fn validate_config(config: &EngineRuntimeConfig, model: &ModelConfig) -> Result<(), String> {
    let e = catalog_entry(config.strata_catalog.as_deref().ok_or("missing Strata catalog selection")?)?;
    let mut base = model.model.clone(); base.serving = None;
    if base != entry_model(&e)? || model.max_concurrency != 1 {
        return Err("Strata model/limits/capabilities differ from curated entry".into());
    }
    let url = parse_origin(&model.engine_url)?;
    if url.scheme() != "http" || !url.host_str().and_then(|s| s.trim_matches(['[', ']']).parse::<IpAddr>().ok()).is_some_and(|ip| ip.is_loopback())
        || url.port().is_none_or(|p| p == 0) || model.engine_api_key_file.is_none() {
        return Err("Strata requires an explicit authenticated loopback endpoint".into());
    }
    if !e["backends"][platform()].as_array().is_some_and(|bs| bs.iter().any(|b| b.as_str() == Some(&config.backend)))
        || (config.backend == "metal" && std::env::consts::ARCH != "aarch64")
        || config.device_ids.len() > 1
        || (config.backend == "cpu" && !config.device_ids.is_empty())
        || (config.backend == "metal" && config.device_ids != ["MTL0"])
        || (config.backend == "cuda" && (config.device_ids.len() != 1
            || config.device_ids[0].strip_prefix("CUDA").is_none_or(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))))
        || config.memory_reserve_mib < if config.backend == "cuda" { 256 } else { 4096 }
        || !(1..=1800).contains(&config.startup_timeout_secs) {
        return Err("unsupported Strata platform/device/headroom configuration".into());
    }
    // Fields shared with legacy signed profiles: fixed values, never interpreted
    // as llama.cpp flags by Strata. Its complete execution recipe is catalog-bound.
    if config.gpu_layers != 0 || config.split_mode != "none" || !config.tensor_split.is_empty()
        || config.cache_type_k != "f16" || config.cache_type_v != "f16" || config.flash_attention
        || config.batch_size != 512 || config.ubatch_size != 512 {
        return Err("Strata requires its fixed catalog execution settings".into());
    }
    Ok(())
}

pub(super) fn validate_inventory(model: &ModelConfig) -> Result<ValidatedRuntime, String> {
    let c = model.runtime.as_ref().ok_or("missing runtime")?;
    validate_config(c, model)?;
    let key = read_secret(model.engine_api_key_file.as_ref().ok_or("missing key")?)?;
    if key.len() < 32 || !key.is_ascii() || key.bytes().any(|b| b.is_ascii_whitespace()) { return Err("invalid Strata engine key".into()); }
    let e = catalog_entry(c.strata_catalog.as_deref().ok_or("missing catalog")?)?;
    let runtime = read_json(&c.runtime_dir.join("chiral-runtime.json"))?;
    if runtime["schema"] != "chiral-strata-runtime-v1" || runtime["source_revision"] != SOURCE_REVISION
        || runtime["engine_version"] != ENGINE_VERSION || runtime["platform"] != platform()
        || runtime["architecture"] != std::env::consts::ARCH || runtime["backend"] != c.backend {
        return Err("Strata runtime platform or pinned identity mismatch".into());
    }
    let files: BTreeMap<String, InventoryFile> = serde_json::from_value(runtime["files"].clone()).map_err(|_| "invalid Strata runtime inventory")?;
    inventory(&c.runtime_dir, &files, &["chiral-runtime.json"])?;
    let expected: BTreeMap<String, String> = serde_json::from_str(UPSTREAM).map_err(|_| "invalid compiled upstream inventory")?;
    for name in files.keys() {
        if !matches!(name.as_str(), "strata-inkling" | "strata-inkling.exe" | "server.py" | "common.py" | "catalog.json")
            && !name.starts_with("python/") && !name.starts_with("lib/")
            && name.strip_prefix("upstream/").is_none_or(|p| !expected.contains_key(p)) {
            return Err("unreviewed Strata runtime support file".into());
        }
    }
    for (name, checksum) in expected {
        if files.get(&format!("upstream/{name}")).is_none_or(|f| f.sha256 != checksum) { return Err("Strata support code differs from reviewed source".into()); }
    }
    for (name, contents) in [("server.py", include_bytes!("../../scripts/strata/server.py").as_slice()),
                             ("common.py", include_bytes!("../../scripts/strata/common.py").as_slice()),
                             ("catalog.json", CATALOG.as_bytes())] {
        if files.get(name).is_none_or(|f| f.sha256 != hex::encode(Sha256::digest(contents))) { return Err("Strata adapter/catalog differs from this Chiral build".into()); }
    }
    let executable_name = if cfg!(windows) { "strata-inkling.exe" } else { "strata-inkling" };
    let python_name = if cfg!(windows) { "python/python.exe" } else { "python/bin/python3" };
    if runtime["executable"] != executable_name || runtime["python"] != python_name
        || !files.contains_key(executable_name) || !files.contains_key(python_name) {
        return Err("Strata entry points are not inventoried native programs".into());
    }
    let executable = c.runtime_dir.join(executable_name).canonicalize().map_err(|_| "missing Strata engine")?;
    let python = c.runtime_dir.join(python_name).canonicalize().map_err(|_| "missing packaged Python")?;
    validate_native_executable(&executable)?; validate_native_executable(&python)?;
    let artifact = read_json(&c.artifact_dir.join("chiral-artifact-sha256.json"))?;
    let expected_artifact = json!({"schema":"chiral-strata-artifact-v1", "catalog_id":e["id"], "files":e["files"],
        "model":{"repo_id":e["repo_id"],"revision":e["revision"]}, "model_file":e["model_file"], "license":e["license"],
        "context_length":e["context_length"],"max_output_tokens":e["max_output_tokens"],"capabilities":e["capabilities"],
        "quantization":e["quantization"], "task":"generation"});
    if artifact != expected_artifact { return Err("Strata artifact differs from curated catalog".into()); }
    let artifact_files = serde_json::from_value(artifact["files"].clone()).map_err(|_| "invalid Strata artifact inventory")?;
    inventory(&c.artifact_dir, &artifact_files, &["chiral-artifact-sha256.json", "chiral-model.json"])?;
    let base: ModelSpec = serde_json::from_value(read_json(&c.artifact_dir.join("chiral-model.json"))?).map_err(|_| "invalid Strata ModelSpec")?;
    if base != entry_model(&e)? { return Err("Strata ModelSpec differs from catalog".into()); }
    let out = ValidatedRuntime { python:Some(python), executable,
        model_file:c.artifact_dir.join(relative_path(e["model_file"].as_str().ok_or("missing model file")?)?),
        template:None, task:"generation".into(), runtime_sha256:manifest_hash(&runtime)?, artifact_manifest_sha256:manifest_hash(&artifact)? };
    if let Some(p) = &model.model.serving {
        if p.engine != "strata" || p.engine_version != ENGINE_VERSION || p.runtime_sha256 != out.runtime_sha256
            || p.artifact_manifest_sha256 != out.artifact_manifest_sha256 || p.hf_repo != e["repo_id"] || p.tokenizer_repo != e["repo_id"]
            || p.platform != platform() || p.backend != c.backend || p.memory_reserve_mib != c.memory_reserve_mib
            || p.gpu_layers != c.gpu_layers || p.split_mode != c.split_mode || p.tensor_split != c.tensor_split
            || p.cache_type_k != c.cache_type_k || p.cache_type_v != c.cache_type_v || p.flash_attention != c.flash_attention
            || p.batch_size != c.batch_size || p.ubatch_size != c.ubatch_size || p.max_concurrency != 1
            || p.devices.iter().map(|d| &d.engine_device).ne(c.device_ids.iter()) {
            return Err("Strata launch differs from immutable serving terms".into());
        }
    }
    Ok(out)
}

pub(super) fn launch_command(model: &ModelConfig, v: &ValidatedRuntime) -> Result<Command, String> {
    let c = model.runtime.as_ref().ok_or("missing runtime")?;
    let url = parse_origin(&model.engine_url)?;
    let mut cmd = command(v.python.as_ref().ok_or("missing packaged Python")?);
    let runtime = c.runtime_dir.canonicalize().map_err(|_| "invalid runtime directory")?;
    let artifacts = c.artifact_dir.canonicalize().map_err(|_| "invalid artifact directory")?;
    cmd.args(["-I", "-B"]).arg(runtime.join("server.py"))
        .arg("--runtime-dir").arg(&runtime).arg("--artifact-dir").arg(artifacts)
        .arg("--catalog-id").arg(c.strata_catalog.as_ref().ok_or("missing catalog")?)
        .arg("--host").arg(url.host_str().ok_or("missing host")?.trim_matches(['[', ']']))
        .arg("--port").arg(url.port().ok_or("missing port")?.to_string())
        .arg("--api-key-file").arg(model.engine_api_key_file.as_ref().ok_or("missing key")?.canonicalize().map_err(|_| "invalid key path")?)
        .arg("--backend").arg(&c.backend).arg("--device")
        .arg(c.device_ids.first().map(|s| s.trim_start_matches("CUDA").trim_start_matches("MTL")).unwrap_or("0"))
        .arg("--memory-reserve-mib").arg(c.memory_reserve_mib.to_string())
        .arg("--startup-timeout").arg(c.startup_timeout_secs.to_string())
        .arg("--request-timeout").arg(model.request_timeout_secs.to_string());
    #[cfg(target_os = "linux")]
    cmd.env("LD_LIBRARY_PATH", runtime.join("lib"));
    Ok(cmd)
}

pub(super) async fn observe_devices(c: &EngineRuntimeConfig, observers: &impl DeviceObservers) -> Result<(String, Vec<EngineDevice>), String> {
    if c.backend == "cpu" { return Ok(("system".into(), vec![])); }
    if c.backend == "cuda" {
        let data = observers.output(Observer::Tool("nvidia-smi"), &NVIDIA_QUERY).await?;
        let mut rows: Vec<_> = data.lines().map(|l| l.split(',').map(str::trim).collect::<Vec<_>>()).collect();
        if rows.iter().any(|r| r.len() != 6) { return Err("invalid CUDA platform observation".into()); }
        rows.sort_by(|a,b| a[0].cmp(b[0]));
        let index: usize = c.device_ids[0].trim_start_matches("CUDA").parse().map_err(|_| "invalid CUDA index")?;
        let row = rows.get(index).ok_or("CUDA device is absent")?;
        return Ok(("dedicated".into(), cuda_devices(&data, vec![(c.device_ids[0].clone(), row[2].into(), 0)], c.memory_reserve_mib)?));
    }
    apple_silicon(&observers.output(Observer::Tool("sysctl"), &["-n", "hw.optional.arm64"]).await?)?;
    let hardware = observers.output(Observer::Tool("ioreg"), &["-rd1", "-c", "IOPlatformExpertDevice"]).await?;
    let id = apple_hardware_id(&hardware)?;
    let memory = apple_memory_mib(&observers.output(Observer::Tool("sysctl"), &["-n", "hw.memsize"]).await?)?;
    apple_available_mib(&observers.output(Observer::Tool("vm_stat"), &[]).await?, memory, c.memory_reserve_mib)?;
    let driver = observers.output(Observer::Tool("sw_vers"), &["-productVersion"]).await?;
    let name = observers.output(Observer::Tool("sysctl"), &["-n", "machdep.cpu.brand_string"]).await?;
    Ok(("unified".into(), vec![EngineDevice { id:id.into(), name:format!("{} integrated GPU", name.trim()), memory_mib:memory,
        driver_version:driver.trim().into(), engine_device:"MTL0".into() }]))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn config() -> ModelConfig {
        ModelConfig { model:entry_model(&catalog_entry("inkling-2-mini-sq-10").unwrap()).unwrap(),
            price:crate::protocol::PriceSchedule { input_wei_per_1k:"0".into(), output_wei_per_1k:"0".into() },
            engine_url:"http://127.0.0.1:18080".into(), engine_api_key_file:Some("/fixture/key".into()),
            max_concurrency:1, request_timeout_secs:5, qualification_evidence_file:None,
            runtime:Some(EngineRuntimeConfig { strata_catalog:Some("inkling-2-mini-sq-10".into()), runtime_dir:"/fixture/runtime".into(), artifact_dir:"/fixture/model".into(),
                backend:"cpu".into(), device_ids:vec![], memory_reserve_mib:4096, gpu_layers:0, split_mode:"none".into(), tensor_split:vec![],
                cache_type_k:"f16".into(), cache_type_v:"f16".into(), flash_attention:false, batch_size:512, ubatch_size:512, startup_timeout_secs:30 }) }
    }

    #[test]
    fn config_cannot_expand_catalog_or_silently_change_execution() {
        let c = config(); c.runtime.as_ref().unwrap().validate(&c).unwrap();
        let mut bad = c.clone(); bad.model.context_length = 8192;
        assert!(bad.runtime.as_ref().unwrap().validate(&bad).is_err());
        let mut bad = c.clone(); bad.max_concurrency = 2;
        assert!(bad.runtime.as_ref().unwrap().validate(&bad).is_err());
        let mut bad = c.clone(); bad.model.capabilities.push("tools".into());
        assert!(bad.runtime.as_ref().unwrap().validate(&bad).is_err());
        let mut bad = c.clone(); bad.runtime.as_mut().unwrap().flash_attention = true;
        assert!(bad.runtime.as_ref().unwrap().validate(&bad).is_err());
        let mut bad = c.clone(); bad.runtime.as_mut().unwrap().strata_catalog = Some("latest".into());
        assert!(bad.runtime.as_ref().unwrap().validate(&bad).is_err());
    }

    #[test]
    fn paid_strata_profiles_require_catalog_and_separate_qualification_identity() {
        let m = config().model;
        let mut p = crate::performance::test_profile();
        p.engine = "strata".into(); p.engine_version = ENGINE_VERSION.into();
        p.hf_repo = "strata-engine/Inkling-2-Mini-Strata".into(); p.tokenizer_repo = p.hf_repo.clone();
        p.gpu_layers = 0; p.flash_attention = false; p.ubatch_size = 512; p.max_concurrency = 1;
        crate::performance::validate_profile(&m, &p).unwrap();
        p.engine_version = super::super::ENGINE_VERSION.into();
        assert!(crate::performance::validate_profile(&m, &p).is_err());
        p.engine_version = ENGINE_VERSION.into(); p.max_concurrency = 2;
        assert!(crate::performance::validate_profile(&m, &p).is_err());
    }
    #[test]
    fn catalog_has_only_two_immutable_text_chat_configurations() {
        for id in ["inkling-2-mini-sq-10", "inkling-2-mini-sq-15"] {
            let e = catalog_entry(id).unwrap(); let m = entry_model(&e).unwrap();
            crate::offer_registry::validate_model(&m).unwrap();
            assert_eq!(m.capabilities, ["chat", "streaming"]);
            assert_eq!(m.context_length, 4096);
            assert_eq!(m.max_output_tokens, 512);
        }
        assert!(catalog_entry("latest").is_err());
        assert!(catalog_entry("arbitrary/gguf").is_err());
    }
}
