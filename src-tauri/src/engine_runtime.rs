//! Pinned, private native llama.cpp processes. No shell, downloads, or inherited engine options.
use crate::{performance::RuntimeObservation, protocol::{self, EngineDevice, ModelSpec}, provider_config::{parse_origin, read_secret, ModelConfig}};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, HashSet}, io::Read, net::IpAddr, path::{Component, Path, PathBuf}, process::Stdio, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}, time::Duration};
use tokio::process::{Child, Command};

pub const ENGINE_VERSION: &str = "v0.9.0";
pub const SOURCE_REVISION: &str = "cd959bae99ff4bed22bef2604f45241c09bbb8c8";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineRuntimeConfig {
    pub runtime_dir: PathBuf,
    pub artifact_dir: PathBuf,
    pub backend: String,
    pub device_ids: Vec<String>,
    pub memory_reserve_mib: u64,
    pub gpu_layers: u32,
    pub split_mode: String,
    pub tensor_split: Vec<u32>,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub flash_attention: bool,
    pub batch_size: u32,
    pub ubatch_size: u32,
    pub startup_timeout_secs: u64,
}

impl EngineRuntimeConfig {
    pub fn validate(&self, model: &ModelConfig) -> Result<(), String> {
        let endpoint = parse_origin(&model.engine_url)?;
        if endpoint.scheme() != "http" || !endpoint.host_str().and_then(|s| s.trim_matches(['[', ']']).parse::<IpAddr>().ok()).is_some_and(|ip| ip.is_loopback()) || endpoint.port().is_none_or(|p| p == 0) {
            return Err("managed engine requires HTTP on an explicit literal loopback address and port".into());
        }
        if model.engine_api_key_file.is_none() { return Err("managed engine requires a protected API key file".into()); }
        if !(1..=1800).contains(&self.startup_timeout_secs) || self.batch_size == 0 || self.batch_size > 8192 || self.ubatch_size == 0 || self.ubatch_size > self.batch_size || self.gpu_layers > 999 {
            return Err("managed engine startup, batch, or layer settings exceed bounds".into());
        }
        let cache = ["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"];
        if !cache.contains(&self.cache_type_k.as_str()) || !cache.contains(&self.cache_type_v.as_str()) || !["none", "layer", "row"].contains(&self.split_mode.as_str()) { return Err("unsupported managed engine cache or split mode".into()); }
        if !self.flash_attention && !["f32", "f16", "bf16"].contains(&self.cache_type_v.as_str()) { return Err("quantized value cache requires flash attention".into()); }
        let prefix = match self.backend.as_str() { "cuda" => "CUDA", "metal" if cfg!(target_os = "macos") => "MTL", "vulkan" => "Vulkan", "cpu" => "", _ => return Err("unsupported native engine backend on this platform".into()) };
        let mut ids = HashSet::new();
        if self.device_ids.len() > 16 || self.device_ids.iter().any(|id| !ids.insert(id) || id.strip_prefix(prefix).is_none_or(|n| n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()))) { return Err("invalid or duplicate explicit engine device selector".into()); }
        if self.backend == "cpu" {
            if self.gpu_layers != 0 || !self.device_ids.is_empty() || !self.tensor_split.is_empty() || self.split_mode != "none" { return Err("CPU runtime cannot select GPU execution".into()); }
        } else if self.device_ids.is_empty() || self.gpu_layers == 0 { return Err("accelerated runtime requires explicit devices and offloaded layers".into()); }
        if (self.device_ids.len() <= 1 && (!self.tensor_split.is_empty() || self.split_mode != "none")) || (self.device_ids.len() > 1 && (self.tensor_split.len() != self.device_ids.len() || self.tensor_split.contains(&0) || self.split_mode == "none")) { return Err("tensor split must describe exactly the selected devices".into()); }
        if (self.backend == "metal" && (self.device_ids.len() != 1 || self.memory_reserve_mib < 4096)) || (matches!(self.backend.as_str(), "cuda" | "vulkan") && self.memory_reserve_mib < 256) { return Err("managed runtime requires explicit platform memory headroom".into()); }
        if model.model.context_length.checked_mul(u64::from(model.max_concurrency)).is_none_or(|n| n > i32::MAX as u64) { return Err("managed aggregate context exceeds engine bounds".into()); }
        Ok(())
    }
}

#[derive(Deserialize)]
struct InventoryFile { sha256: String, size_bytes: u64 }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeManifest {
    schema: String, platform: String, architecture: String, backend: String,
    engine_version: String, source_revision: String, executable: String,
    files: BTreeMap<String, InventoryFile>,
}

pub struct ValidatedRuntime {
    executable: PathBuf,
    model_file: PathBuf,
    template: Option<PathBuf>,
    task: String,
    runtime_sha256: String,
    artifact_manifest_sha256: String,
}

fn relative_path(raw: &str) -> Result<&Path, String> {
    let path = Path::new(raw);
    if raw.is_empty() || raw.contains('\\') || raw.contains(':') || path.components().any(|c| !matches!(c, Component::Normal(_))) { return Err("inventory path is not a safe relative path".into()); }
    Ok(path)
}
fn read_json(path: &Path) -> Result<Value, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "cannot inspect runtime metadata")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4 * 1024 * 1024 { return Err("runtime metadata must be a bounded regular file".into()); }
    serde_json::from_slice(&std::fs::read(path).map_err(|_| "cannot read runtime metadata")?).map_err(|_| "invalid runtime metadata".into())
}
fn manifest_hash(value: &Value) -> Result<String, String> { Ok(hex::encode(Sha256::digest(protocol::canonical_json(value)?))) }
fn inventory(root: &Path, files: &BTreeMap<String, InventoryFile>, metadata: &[&str]) -> Result<(), String> {
    if !std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) || files.is_empty() || files.len() > 8192 { return Err("invalid inventory directory or file count".into()); }
    let mut expected: HashSet<PathBuf> = metadata.iter().map(PathBuf::from).collect();
    for (name, entry) in files {
        let relative = relative_path(name)?;
        if !expected.insert(relative.to_owned()) || entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err("invalid inventory digest or reserved filename".into()); }
    }
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).map_err(|_| "cannot enumerate immutable inventory")? {
            let entry = entry.map_err(|_| "cannot inspect inventory entry")?;
            let kind = entry.file_type().map_err(|_| "cannot inspect inventory entry")?;
            if kind.is_symlink() { return Err("inventory symlinks are forbidden".into()); }
            if kind.is_dir() { pending.push(entry.path()); continue; }
            if !kind.is_file() || !expected.remove(entry.path().strip_prefix(root).map_err(|_| "inventory escaped root")?) { return Err("unexpected file in immutable inventory".into()); }
        }
    }
    if !expected.is_empty() { return Err("immutable inventory file is missing".into()); }
    let mut buffer = [0u8; 128 * 1024];
    for (name, entry) in files {
        let path = root.join(relative_path(name)?);
        let mut file = std::fs::File::open(path).map_err(|_| "cannot read inventory file")?;
        if file.metadata().map_err(|_| "cannot inspect inventory file")?.len() != entry.size_bytes { return Err("inventory file size mismatch".into()); }
        let mut digest = Sha256::new();
        loop { let n = file.read(&mut buffer).map_err(|_| "cannot hash inventory file")?; if n == 0 { break; } digest.update(&buffer[..n]); }
        if hex::encode(digest.finalize()) != entry.sha256 { return Err("inventory file digest mismatch".into()); }
    }
    Ok(())
}

fn template_digest(source: &[u8]) -> String {
    // Match the pinned Jinja lexer's effective source, while the inventory binds
    // the untouched GGUF-derived file. Do not trim meaningful whitespace.
    let source = source.strip_suffix(b"\r\n").or_else(|| source.strip_suffix(b"\n")).unwrap_or(source);
    let mut segments = source.split(|byte| *byte == b'\r');
    let mut digest = Sha256::new();
    digest.update(segments.next().unwrap_or_default());
    for segment in segments {
        digest.update(b"\n");
        digest.update(segment.strip_prefix(b"\n").unwrap_or(segment));
    }
    hex::encode(digest.finalize())
}

pub fn validate_inventory(model: &ModelConfig) -> Result<ValidatedRuntime, String> {
    let config = model.runtime.as_ref().ok_or("model is not managed")?;
    config.validate(model)?;
    let key_path = model.engine_api_key_file.as_ref().ok_or("missing engine key")?;
    let key = read_secret(key_path)?;
    // Upstream reads one literal key per line, unlike read_secret's trimming behavior.
    let raw = std::fs::read(key_path).map_err(|_| "cannot read engine key")?;
    if key.len() < 32 || key.starts_with('#') || !(raw == key.as_bytes() || raw == format!("{key}\n").as_bytes()) { return Err("engine key must be one literal token of at least 32 characters".into()); }
    let runtime_value = read_json(&config.runtime_dir.join("chiral-runtime.json"))?;
    let runtime: RuntimeManifest = serde_json::from_value(runtime_value.clone()).map_err(|_| "invalid runtime manifest")?;
    if runtime.schema != "chiral-llama-runtime-v1" || runtime.platform != platform() || runtime.architecture != std::env::consts::ARCH || runtime.backend != config.backend || runtime.engine_version != ENGINE_VERSION || runtime.source_revision != SOURCE_REVISION { return Err("runtime platform, backend, or pinned upstream identity mismatch".into()); }
    inventory(&config.runtime_dir, &runtime.files, &["chiral-runtime.json"])?;
    if !runtime.files.contains_key(&runtime.executable) { return Err("runtime executable is not inventoried".into()); }
    let executable = config.runtime_dir.join(relative_path(&runtime.executable)?).canonicalize().map_err(|_| "cannot resolve runtime executable")?;
    if executable.file_name().and_then(|s| s.to_str()) != Some(if cfg!(windows) { "llama-server.exe" } else { "llama-server" }) { return Err("runtime entry point must be native llama-server".into()); }
    validate_native_executable(&executable)?;
    let artifact = read_json(&config.artifact_dir.join("chiral-artifact-sha256.json"))?;
    let files: BTreeMap<String, InventoryFile> = serde_json::from_value(artifact.get("files").cloned().ok_or("missing artifact inventory")?).map_err(|_| "invalid artifact inventory")?;
    inventory(&config.artifact_dir, &files, &["chiral-artifact-sha256.json", "chiral-model.json"])?;
    let base: ModelSpec = serde_json::from_value(read_json(&config.artifact_dir.join("chiral-model.json"))?).map_err(|_| "invalid artifact ModelSpec")?;
    let mut expected = model.model.clone(); expected.serving = None;
    if base != expected || base.tokenizer_revision != base.revision || artifact["schema"] != "chiral-gguf-artifact-v1" || artifact["model"]["revision"] != base.revision || artifact["quantization"].as_str() != base.quantization.as_deref() || artifact["context_length"].as_u64() != Some(base.context_length) || artifact["max_output_tokens"].as_u64() != Some(base.max_output_tokens) || artifact["capabilities"] != serde_json::to_value(&base.capabilities).map_err(|_| "invalid capabilities")? { return Err("artifact identity differs from configured ModelSpec".into()); }
    let task = artifact["task"].as_str().ok_or("missing artifact task")?;
    if !matches!(task, "generation" | "embeddings") || (task == "embeddings") != base.capabilities.iter().any(|c| c == "embeddings") || (task == "embeddings" && base.capabilities.iter().any(|c| matches!(c.as_str(), "chat" | "completions"))) { return Err("artifact task and model capabilities disagree".into()); }
    let model_name = artifact["model_file"].as_str().ok_or("missing GGUF entry point")?;
    if !model_name.ends_with(".gguf") || !files.contains_key(model_name) { return Err("GGUF entry point is not inventoried".into()); }
    let repo = artifact["model"]["repo_id"].as_str().ok_or("missing model repository")?;
    if base.id != format!("{repo}/{}", relative_path(model_name)?.file_name().and_then(|s| s.to_str()).ok_or("invalid GGUF filename")?) { return Err("model API alias differs from selected GGUF identity".into()); }
    let template_entry = files.get("chiral-chat-template.jinja");
    if task == "embeddings"
        && (artifact.get("chat_template_name") != Some(&Value::Null) || template_entry.is_some())
    {
        return Err("embedding artifacts require null template selection and no template file".into());
    }
    let template = if template_entry.is_some() { Some(config.artifact_dir.join("chiral-chat-template.jinja").canonicalize().map_err(|_| "cannot resolve selected template")?) } else { None };
    if template_entry.is_some_and(|entry| entry.size_bytes > 1024 * 1024) { return Err("selected template exceeds 1 MiB".into()); }
    let template_bytes = match &template { Some(path) => std::fs::read(path).map_err(|_| "cannot read template")?, None => Vec::new() };
    if std::str::from_utf8(&template_bytes).is_err() || template_digest(&template_bytes) != base.chat_template_sha256 || (base.capabilities.iter().any(|c| c == "chat") && template.is_none()) { return Err("selected chat template identity mismatch".into()); }
    let validated = ValidatedRuntime { executable, model_file: config.artifact_dir.join(relative_path(model_name)?).canonicalize().map_err(|_| "cannot resolve GGUF model")?, template, task: task.into(), runtime_sha256: manifest_hash(&runtime_value)?, artifact_manifest_sha256: manifest_hash(&artifact)? };
    if let Some(profile) = &model.model.serving {
        if profile.engine != "llama.cpp" || profile.engine_version != ENGINE_VERSION || profile.runtime_sha256 != validated.runtime_sha256 || profile.artifact_manifest_sha256 != validated.artifact_manifest_sha256 || profile.hf_repo != repo || profile.tokenizer_repo != repo || profile.platform != platform() || profile.backend != config.backend || profile.memory_reserve_mib != config.memory_reserve_mib || profile.gpu_layers != config.gpu_layers || profile.split_mode != config.split_mode || profile.tensor_split != config.tensor_split || profile.cache_type_k != config.cache_type_k || profile.cache_type_v != config.cache_type_v || profile.flash_attention != config.flash_attention || profile.batch_size != config.batch_size || profile.ubatch_size != config.ubatch_size || profile.max_concurrency != model.max_concurrency || profile.devices.iter().map(|d| &d.engine_device).ne(config.device_ids.iter()) { return Err("managed launch settings differ from immutable serving terms".into()); }
    }
    Ok(validated)
}

fn validate_native_executable(path: &Path) -> Result<(), String> {
    let mut header = [0u8; 4096];
    let n = std::fs::File::open(path).map_err(|_| "cannot open native executable")?
        .read(&mut header).map_err(|_| "cannot inspect native executable")?;
    let header = &header[..n];
    let arm = std::env::consts::ARCH == "aarch64";
    let valid = if cfg!(target_os = "linux") {
        header.len() >= 20 && &header[..6] == b"\x7fELF\x02\x01"
            && u16::from_le_bytes([header[18], header[19]]) == if arm { 183 } else { 62 }
    } else if cfg!(windows) {
        if header.len() < 64 || &header[..2] != b"MZ" { false } else {
            let offset = u32::from_le_bytes(header[60..64].try_into().map_err(|_| "invalid PE header")?) as usize;
            offset.checked_add(6).is_some_and(|end| end <= header.len())
                && &header[offset..offset + 4] == b"PE\0\0"
                && u16::from_le_bytes([header[offset + 4], header[offset + 5]]) == if arm { 0xaa64 } else { 0x8664 }
        }
    } else if cfg!(target_os = "macos") {
        header.len() >= 8 && &header[..4] == b"\xcf\xfa\xed\xfe"
            && u32::from_le_bytes(header[4..8].try_into().map_err(|_| "invalid Mach-O header")?) == if arm { 0x0100_000c } else { 0x0100_0007 }
    } else { false };
    if !valid { return Err("runtime entry point is not a native executable for this platform".into()); }
    Ok(())
}

fn platform() -> &'static str { if cfg!(target_os = "macos") { "macos" } else { std::env::consts::OS } }
fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.env_clear().stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    command.env("CUDA_DEVICE_ORDER", "PCI_BUS_ID");
    // OS loader/temporary-directory necessities only; no PATH, HOME, proxy, wallet,
    // LLAMA_ARG_*, CUDA visibility masks, or dynamic-library injection variables.
    for name in ["SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] { if let Some(value) = std::env::var_os(name) { command.env(name, value); } }
    command.current_dir(executable.parent().unwrap_or_else(|| Path::new("/")));
    #[cfg(unix)]
    { command.process_group(0); }
    #[cfg(target_os = "linux")]
    unsafe {
        let parent = libc::getpid();
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 || libc::getppid() != parent { return Err(std::io::Error::last_os_error()); }
            Ok(())
        });
    }
    command
}
fn launch_command(model: &ModelConfig, validated: &ValidatedRuntime) -> Result<Command, String> {
    let config = model.runtime.as_ref().ok_or("model is not managed")?;
    let endpoint = parse_origin(&model.engine_url)?;
    let mut command = command(&validated.executable);
    command.arg("--model").arg(&validated.model_file).arg("--alias").arg(&model.model.id)
        .arg("--host").arg(endpoint.host_str().ok_or("missing host")?.trim_matches(['[', ']']))
        .arg("--port").arg(endpoint.port().ok_or("missing port")?.to_string())
        .arg("--api-key-file").arg(model.engine_api_key_file.as_ref().ok_or("missing key")?.canonicalize().map_err(|_| "cannot resolve engine key")?)
        .arg("--ctx-size").arg((model.model.context_length * u64::from(model.max_concurrency)).to_string())
        .arg("--kv-unified-per-slot").arg(model.model.context_length.to_string())
        .arg("--parallel").arg(model.max_concurrency.to_string())
        .arg("--n-predict").arg(model.model.max_output_tokens.to_string())
        .args(["--no-context-shift", "--no-cache-prompt", "--cache-reuse", "0", "--cache-ram", "0", "--fit", "off", "--no-webui", "--no-mmproj", "--no-prefill-assistant", "--log-disable"])
        .arg("--device").arg(if config.device_ids.is_empty() { "none".into() } else { config.device_ids.join(",") })
        .arg("--gpu-layers").arg(config.gpu_layers.to_string()).arg("--split-mode").arg(&config.split_mode)
        .arg("--cache-type-k").arg(&config.cache_type_k).arg("--cache-type-v").arg(&config.cache_type_v)
        .arg("--flash-attn").arg(if config.flash_attention { "on" } else { "off" })
        .arg("--batch-size").arg(config.batch_size.to_string()).arg("--ubatch-size").arg(config.ubatch_size.to_string());
    if !config.tensor_split.is_empty() { command.arg("--tensor-split").arg(config.tensor_split.iter().map(u32::to_string).collect::<Vec<_>>().join(",")); }
    if validated.task == "embeddings" { command.arg("--embedding"); }
    if let Some(template) = &validated.template { command.arg("--jinja").arg("--chat-template-file").arg(template); }
    Ok(command)
}

const OCCUPIED_ENGINE_PORT: &str = "managed engine loopback port is already occupied";

/// Half of the occupied-port check (requirement 4, design decision 5). The endpoint listener
/// that `ManagedEngine::start` also binds uses `SO_REUSEADDR` (mio sets it on Unix), with which
/// macOS binds `127.0.0.1:P` beside another process listening on `0.0.0.0:P`. This socket binds,
/// without listening, the wildcard address of the endpoint's family on the same port, which such
/// a listener refuses. It keeps `SO_REUSEADDR` on Unix, because without it a port that holds
/// only expiring `TIME_WAIT` connections (a stopped engine's) is refused too. On Windows the
/// option would let the probe share a port another socket holds, so, as mio does, it is not set
/// there. An IPv6 probe is `IPV6_V6ONLY`, so an IPv4-only wildcard listener does not refuse an
/// IPv6 endpoint. It is bound before the endpoint listener: Linux refuses a wildcard bind on a
/// port where a socket listens on a specific address, the launcher's own reservation included,
/// but not a specific-address bind beside a wildcard socket that does not listen.
fn wildcard_port_probe(endpoint: std::net::SocketAddr) -> std::io::Result<tokio::net::TcpSocket> {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    let (socket, wildcard) = match endpoint {
        SocketAddr::V4(_) => (tokio::net::TcpSocket::new_v4()?, SocketAddr::from((Ipv4Addr::UNSPECIFIED, endpoint.port()))),
        SocketAddr::V6(_) => (tokio::net::TcpSocket::new_v6()?, SocketAddr::from((Ipv6Addr::UNSPECIFIED, endpoint.port()))),
    };
    #[cfg(not(windows))]
    socket.set_reuseaddr(true)?;
    #[cfg(unix)]
    if endpoint.is_ipv6() {
        use std::os::fd::AsRawFd;
        let on: libc::c_int = 1;
        // SAFETY: a valid option pointer and length for an open socket descriptor.
        if unsafe { libc::setsockopt(socket.as_raw_fd(), libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, (&on as *const libc::c_int).cast(), std::mem::size_of::<libc::c_int>() as libc::socklen_t) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    socket.bind(wildcard)?;
    Ok(socket)
}

pub struct ManagedEngine {
    child: Mutex<Child>,
    ready: AtomicBool,
    pub observation: RuntimeObservation,
    #[cfg(windows)]
    _job: WindowsJob,
}
impl ManagedEngine {
    pub async fn start(model: &ModelConfig) -> Result<Arc<Self>, String> {
        Self::start_observed(model, &SystemObservers).await
    }
    /// `start` with the given device observers: the system's, or recorded output in tests.
    async fn start_observed(model: &ModelConfig, observers: &impl DeviceObservers) -> Result<Arc<Self>, String> {
        #[cfg(test)]
        LAUNCH_ATTEMPTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(model.engine_url.clone());
        let owned = model.clone();
        let validated = tokio::task::spawn_blocking(move || validate_inventory(&owned)).await.map_err(|_| "inventory worker stopped")??;
        let config = model.runtime.as_ref().ok_or("model is not managed")?;
        let (memory_kind, devices) = observe_devices(config, &validated.executable, observers).await?;
        if let Some(profile) = &model.model.serving { if profile.memory_kind != memory_kind || profile.devices != devices { return Err("observed physical devices differ from qualified serving inventory".into()); } }
        // Refuse an already occupied endpoint rather than accidentally qualifying an unrelated process.
        let endpoint = parse_origin(&model.engine_url)?;
        let address = endpoint.socket_addrs(|| None).map_err(|_| "invalid engine bind address")?.into_iter().next().ok_or("missing engine bind address")?;
        let wildcard = wildcard_port_probe(address).map_err(|_| OCCUPIED_ENGINE_PORT)?;
        let reservation = tokio::net::TcpListener::bind(address).await.map_err(|_| OCCUPIED_ENGINE_PORT)?;
        let mut launch = launch_command(model, &validated)?;
        drop(reservation);
        drop(wildcard);
        let child = launch.spawn().map_err(|_| "cannot start pinned native llama-server")?;
        let pid = child.id().ok_or("managed engine did not acquire process identity")?;
        #[cfg(windows)]
        let job = WindowsJob::attach(pid)?;
        let engine = Arc::new(Self {
            child: Mutex::new(child), ready: AtomicBool::new(false),
            observation: RuntimeObservation { pid, model_id: model.model.id.clone(), engine_url: model.engine_url.clone(), context_length: model.model.context_length, max_concurrency: model.max_concurrency, artifact_manifest_sha256: validated.artifact_manifest_sha256, runtime_sha256: validated.runtime_sha256, engine_version: ENGINE_VERSION.into(), platform: platform().into(), backend: config.backend.clone(), memory_kind, devices, memory_reserve_mib: config.memory_reserve_mib, gpu_layers: config.gpu_layers, split_mode: config.split_mode.clone(), tensor_split: config.tensor_split.clone(), cache_type_k: config.cache_type_k.clone(), cache_type_v: config.cache_type_v.clone(), flash_attention: config.flash_attention, batch_size: config.batch_size, ubatch_size: config.ubatch_size },
            #[cfg(windows)]
            _job: job,
        });
        engine.await_loaded(model, &validated.executable, observers).await
    }
    /// The launch after the spawn: the engine's health, then the devices observed again, and only
    /// then readiness. An error drops `self`, the only reference to the new engine, which stops
    /// the child (`Drop`).
    async fn await_loaded(self: Arc<Self>, model: &ModelConfig, executable: &Path, observers: &impl DeviceObservers) -> Result<Arc<Self>, String> {
        let config = model.runtime.as_ref().ok_or("model is not managed")?;
        let token = read_secret(model.engine_api_key_file.as_ref().ok_or("missing engine key")?)?;
        self.await_health(&model.engine_url, &token, Duration::from_secs(config.startup_timeout_secs)).await?;
        // Check free memory after weights and KV buffers have actually been allocated.
        let (kind, devices) = observe_devices(config, executable, observers).await?;
        if kind != self.observation.memory_kind || devices != self.observation.devices || !self.process_alive() {
            return Err("runtime device identity changed during startup".into());
        }
        self.ready.store(true, Ordering::Release);
        Ok(self)
    }
    async fn await_health(&self, engine_url: &str, token: &str, timeout: Duration) -> Result<(), String> {
        let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(2)).build().map_err(|_| "cannot initialize private readiness probe")?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if !self.process_alive() { return Err("managed engine exited before readiness".into()); }
            let health = client.get(format!("{}/health", engine_url.trim_end_matches('/'))).bearer_auth(token).send();
            match tokio::time::timeout_at(deadline, health).await {
                Ok(Ok(response)) if response.status().is_success() && self.process_alive() => return Ok(()),
                _ => {}
            }
            if tokio::time::Instant::now() >= deadline { self.stop().await; return Err("managed engine startup timed out".into()); }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    fn process_alive(&self) -> bool { self.child.lock().is_ok_and(|mut child| matches!(child.try_wait(), Ok(None))) }
    pub fn is_alive(&self) -> bool {
        if !self.ready.load(Ordering::Acquire) { return false; }
        if !self.process_alive() { self.ready.store(false, Ordering::Release); return false; }
        true
    }
    fn terminate(&self) {
        self.ready.store(false, Ordering::Release);
        if let Ok(mut child) = self.child.lock() {
            if let Some(pid) = child.id() {
                #[cfg(unix)]
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL); }
                #[cfg(not(unix))]
                let _ = pid;
            }
            let _ = child.start_kill();
        }
    }
    pub async fn stop(&self) {
        self.terminate();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while self.process_alive() && tokio::time::Instant::now() < deadline { tokio::time::sleep(Duration::from_millis(20)).await; }
    }
}
impl Drop for ManagedEngine { fn drop(&mut self) { self.terminate(); } }

/// Test seam (`managed-engine-runtime` tasks 3.9 and 3.10): the engine endpoint of every launch
/// attempt, recorded before the launch checks anything, so that a provider test can show that
/// nothing attempted to start an engine again, whatever the model's configuration.
#[cfg(test)]
static LAUNCH_ATTEMPTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(test)]
impl ManagedEngine {
    /// The launch attempts (`start` calls) with `engine_url` so far in this test process.
    pub(crate) fn launch_attempts_for_test(engine_url: &str) -> usize {
        LAUNCH_ATTEMPTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().filter(|url| *url == engine_url).count()
    }
    /// `managed-engine-runtime` task 3.10's test constructor: this test binary's ignored
    /// `managed_child_helper`, which only sleeps, wrapped as a ready managed engine for
    /// `model_id` at `engine_url`. No inventory, device or health check runs.
    pub(crate) fn helper_for_test(model_id: &str, engine_url: &str) -> Self {
        Self::helper_running_for_test(model_id, engine_url, &["--exact", "engine_runtime::tests::managed_child_helper", "--ignored", "--nocapture"])
    }
    /// `helper_for_test` whose child is `recording_child_helper`: it appends its process id
    /// to `spawn_log` when it starts, then sleeps, so that a test can count the engine
    /// processes started for the model (`managed-engine-runtime` task 3.10).
    pub(crate) fn recording_helper_for_test(model_id: &str, engine_url: &str, spawn_log: &Path) -> Self {
        let log = format!("spawn-log={}", spawn_log.display());
        Self::helper_running_for_test(model_id, engine_url, &tests::helper_args("engine_runtime::tests::recording_child_helper").into_iter().chain([log.as_str()]).collect::<Vec<_>>())
    }
    /// `helper_for_test` running this test binary with `args`, through the launcher's own
    /// `command` (process group, Linux parent-death signal, cleared environment) and, on
    /// Windows, its job object.
    pub(crate) fn helper_running_for_test(model_id: &str, engine_url: &str, args: &[&str]) -> Self {
        let mut launch = command(&std::env::current_exe().unwrap());
        launch.args(args);
        let child = launch.spawn().unwrap();
        let pid = child.id().unwrap();
        Self { child:Mutex::new(child), ready:AtomicBool::new(true), observation:RuntimeObservation {
            pid, engine_url:engine_url.into(), model_id:model_id.into(), context_length:512, max_concurrency:1,
            artifact_manifest_sha256:"00".repeat(32), runtime_sha256:"11".repeat(32), engine_version:ENGINE_VERSION.into(),
            platform:platform().into(), backend:"cpu".into(), memory_kind:"system".into(), devices:vec![], memory_reserve_mib:0,
            gpu_layers:0, split_mode:"none".into(), tensor_split:vec![], cache_type_k:"f16".into(), cache_type_v:"f16".into(), flash_attention:false, batch_size:128, ubatch_size:64 },
            #[cfg(windows)]
            _job:WindowsJob::attach(pid).unwrap(),
        }
    }
    /// Ends the helper child as an engine crash would, without the daemon's `stop`.
    pub(crate) fn kill_for_test(&self) {
        let _ = self.child.lock().unwrap().start_kill();
    }
}

// Device observation helpers never use configured names, UUIDs, memory sizes or driver claims.
async fn capture(executable: &Path, args: &[&str]) -> Result<String, String> {
    capture_within(executable, args, OBSERVER_TIMEOUT).await
}
/// How long one device observer may run.
const OBSERVER_TIMEOUT: Duration = Duration::from_secs(10);
async fn capture_within(executable: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut command = command(executable);
    command.args(args).stdout(Stdio::piped());
    let mut child = command.spawn().map_err(|_| "platform device observer is unavailable")?;
    let mut stdout = child.stdout.take().ok_or("device observer output unavailable")?;
    use tokio::io::AsyncReadExt;
    let result = tokio::time::timeout(timeout, async {
        let mut bytes = Vec::new();
        (&mut stdout).take(1024 * 1024 + 1).read_to_end(&mut bytes).await.map_err(|_| "device observation read failed")?;
        if bytes.len() > 1024 * 1024 { return Err("device observation exceeded size bound"); }
        if !child.wait().await.map_err(|_| "device observer wait failed")?.success() { return Err("device observer failed"); }
        String::from_utf8(bytes).map_err(|_| "device observation is not UTF-8")
    }).await;
    match result { Ok(Ok(value)) => Ok(value), _ => { let _ = child.start_kill(); Err("platform device observation failed or timed out".into()) } }
}
fn system_tool(name: &str) -> Result<PathBuf, String> {
    #[cfg(windows)]
    let roots = { let root = std::env::var_os("SystemRoot").ok_or("missing Windows system directory")?; vec![PathBuf::from(&root).join("System32"), PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\NVSMI")] };
    #[cfg(not(windows))]
    let roots = vec![PathBuf::from("/usr/bin"), PathBuf::from("/usr/sbin"), PathBuf::from("/bin"), PathBuf::from("/sbin")];
    roots.into_iter().map(|root| root.join(if cfg!(windows) { format!("{name}.exe") } else { name.into() })).find(|path| path.is_file()).ok_or_else(|| format!("required platform observer {name} is unavailable"))
}

/// A device observer: the verified engine's own enumeration, or a platform tool at a fixed
/// system location (`system_tool`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observer<'a> { Engine(&'a Path), Tool(&'static str) }

/// Runs device observers and returns their standard output. `SystemObservers` runs the real
/// programs; tests replay recorded output (`managed-engine-runtime` task 3.7).
trait DeviceObservers: Sync {
    fn output<'a>(&'a self, observer: Observer<'a>, args: &'a [&'a str]) -> impl std::future::Future<Output = Result<String, String>> + Send + 'a;
}

struct SystemObservers;
impl DeviceObservers for SystemObservers {
    fn output<'a>(&'a self, observer: Observer<'a>, args: &'a [&'a str]) -> impl std::future::Future<Output = Result<String, String>> + Send + 'a {
        async move {
            match observer {
                Observer::Engine(executable) => capture(executable, args).await,
                Observer::Tool(name) => capture(&system_tool(name)?, args).await,
            }
        }
    }
}

/// A selected device in the engine's enumeration: its selector, name and total memory in MiB.
type ListedDevice = (String, String, u64);

/// The selected devices in the engine's `--list-devices` output, in selector order, each listed
/// once with free memory of at least `reserve` MiB and total memory above it.
fn engine_devices(listed: &str, selectors: &[String], reserve: u64) -> Result<Vec<ListedDevice>, String> {
    let mut observed = Vec::new();
    for selector in selectors {
        let mut lines = listed.lines().filter_map(|line| line.trim().strip_prefix(&format!("{selector}: ")));
        let line = lines.next().ok_or("selected device is absent from pinned engine enumeration")?;
        // Ambiguous output fails the launch (requirement 7); a selector is listed once.
        if lines.next().is_some() { return Err("selected device is listed more than once by pinned engine enumeration".into()); }
        let (name, memory) = line.rsplit_once(" (").ok_or("invalid engine device observation")?;
        let (total, free) = memory.trim_end_matches(')').split_once(" MiB, ").ok_or("invalid engine device memory")?;
        let total = total.parse::<u64>().map_err(|_| "invalid engine device capacity")?;
        let free = free.trim_end_matches(" MiB free").parse::<u64>().map_err(|_| "invalid engine device free memory")?;
        if free < reserve || total <= reserve { return Err("selected engine device lacks required memory headroom".into()); }
        observed.push((selector.clone(), name.to_owned(), total));
    }
    Ok(observed)
}

const NVIDIA_QUERY: [&str; 2] = ["--query-gpu=pci.bus_id,uuid,name,memory.total,driver_version,memory.free", "--format=csv,noheader,nounits"];

/// The CUDA devices from `nvidia-smi` output (`NVIDIA_QUERY`): each selector `CUDA<n>` is the
/// `n`th row in PCI bus order (no bus ID listed twice), whose name equals the engine's and whose
/// free memory is at least `reserve` MiB.
fn cuda_devices(data: &str, observed: Vec<ListedDevice>, reserve: u64) -> Result<Vec<EngineDevice>, String> {
    let mut rows: Vec<_> = data.lines().map(|line| line.split(',').map(str::trim).collect::<Vec<_>>()).collect();
    if rows.iter().any(|row| row.len() != 6) { return Err("invalid CUDA platform observation".into()); }
    rows.sort_by(|a, b| a[0].cmp(b[0]));
    // Ambiguous output fails the launch (requirement 7): two GPUs on one bus ID have no order.
    if rows.windows(2).any(|pair| pair[0][0] == pair[1][0]) { return Err("CUDA physical identity is ambiguous".into()); }
    let mut devices = Vec::new();
    for (selector, name, _) in observed {
        let index = selector.trim_start_matches("CUDA").parse::<usize>().map_err(|_| "invalid CUDA device index")?;
        let row = rows.get(index).ok_or("CUDA physical identity unavailable")?;
        let memory = row[3].parse::<u64>().map_err(|_| "invalid CUDA memory observation")?;
        if row[2] != name || row[5].parse::<u64>().map_err(|_| "invalid CUDA free memory")? < reserve { return Err("CUDA engine and platform device observations disagree".into()); }
        devices.push(EngineDevice { id: row[1].into(), name, memory_mib: memory, driver_version: row[4].into(), engine_device: selector });
    }
    Ok(devices)
}

/// `sysctl -n hw.optional.arm64`: unified Metal memory is observed only on Apple silicon.
fn apple_silicon(arm64: &str) -> Result<(), String> {
    if arm64.trim() != "1" {
        return Err("unified Metal observation requires Apple silicon; discrete Metal memory is not qualified".into());
    }
    Ok(())
}

/// The `IOPlatformUUID` of `ioreg -rd1 -c IOPlatformExpertDevice`.
fn apple_hardware_id(hardware: &str) -> Result<&str, String> {
    Ok(hardware.lines().find_map(|line| line.split_once("\"IOPlatformUUID\" = \"").and_then(|(_, value)| value.strip_suffix('"'))).ok_or("Apple physical hardware identity unavailable")?)
}

/// The physical memory in MiB of `sysctl -n hw.memsize`.
fn apple_memory_mib(memsize: &str) -> Result<u64, String> {
    Ok(memsize.trim().parse::<u64>().map_err(|_| "invalid Apple unified memory observation")? / (1024 * 1024))
}

/// The available memory of `vm_stat`, free, inactive and speculative pages in MiB, capped at
/// `memory` MiB, which must be at least `reserve` MiB.
fn apple_available_mib(vm: &str, memory: u64, reserve: u64) -> Result<u64, String> {
    let page_size = vm.lines().next().and_then(|line| line.split_once("page size of "))
        .and_then(|(_, rest)| rest.split_whitespace().next()).and_then(|n| n.parse::<u64>().ok())
        .ok_or("Apple VM page size unavailable")?;
    let mut available_pages = 0u64;
    for field in ["Pages free", "Pages inactive", "Pages speculative"] {
        let pages = vm.lines().find_map(|line| line.split_once(':').filter(|(key, _)| *key == field)
            .and_then(|(_, value)| value.trim().trim_end_matches('.').parse::<u64>().ok()))
            .ok_or("Apple available memory observation unavailable")?;
        available_pages = available_pages.checked_add(pages).ok_or("Apple memory observation overflow")?;
    }
    let available = available_pages.checked_mul(page_size).ok_or("Apple memory observation overflow")? / (1024 * 1024);
    if available.min(memory) < reserve { return Err("Apple unified memory lacks required headroom".into()); }
    Ok(available.min(memory))
}

/// The Vulkan devices from `vulkaninfo --summary`: each engine device is the one discrete GPU
/// of the same name, with the engine's total memory.
fn vulkan_devices(summary: &str, observed: Vec<ListedDevice>) -> Result<Vec<EngineDevice>, String> {
    let mut records = Vec::<BTreeMap<String, String>>::new();
    for line in summary.lines() {
        if line.trim().starts_with("GPU") && line.trim().ends_with(':') { records.push(BTreeMap::new()); }
        if let Some((key, value)) = line.trim().split_once('=') { if let Some(record) = records.last_mut() { record.insert(key.trim().into(), value.trim().into()); } }
    }
    let mut devices = Vec::new();
    for (selector, name, total) in observed {
        let rows: Vec<_> = records.iter().filter(|row| row.get("deviceName") == Some(&name)).collect();
        if rows.len() != 1 { return Err("Vulkan physical identity is absent or ambiguous".into()); }
        let row = rows[0];
        if row.get("deviceType").is_none_or(|v| v != "PHYSICAL_DEVICE_TYPE_DISCRETE_GPU") { return Err("Vulkan unified/system memory observation is unsupported; use native Metal or CPU".into()); }
        devices.push(EngineDevice { id: row.get("deviceUUID").ok_or("Vulkan UUID unavailable")?.clone(), name, memory_mib: total, driver_version: row.get("driverInfo").or_else(|| row.get("driverVersion")).ok_or("Vulkan driver identity unavailable")?.clone(), engine_device: selector });
    }
    Ok(devices)
}

/// Runs the observers in order, each only after the previous output passed its checks, and
/// parses their output with the functions above.
async fn observe_devices(config: &EngineRuntimeConfig, executable: &Path, observers: &impl DeviceObservers) -> Result<(String, Vec<EngineDevice>), String> {
    if config.backend == "cpu" { return Ok(("system".into(), Vec::new())); }
    let listed = observers.output(Observer::Engine(executable), &["--list-devices"]).await?;
    let observed = engine_devices(&listed, &config.device_ids, config.memory_reserve_mib)?;
    if config.backend == "cuda" {
        let data = observers.output(Observer::Tool("nvidia-smi"), &NVIDIA_QUERY).await?;
        return Ok(("dedicated".into(), cuda_devices(&data, observed, config.memory_reserve_mib)?));
    }
    if config.backend == "metal" {
        apple_silicon(&observers.output(Observer::Tool("sysctl"), &["-n", "hw.optional.arm64"]).await?)?;
        let hardware = observers.output(Observer::Tool("ioreg"), &["-rd1", "-c", "IOPlatformExpertDevice"]).await?;
        let id = apple_hardware_id(&hardware)?;
        let memory = apple_memory_mib(&observers.output(Observer::Tool("sysctl"), &["-n", "hw.memsize"]).await?)?;
        apple_available_mib(&observers.output(Observer::Tool("vm_stat"), &[]).await?, memory, config.memory_reserve_mib)?;
        let driver = observers.output(Observer::Tool("sw_vers"), &["-productVersion"]).await?;
        let (selector, name, _) = observed.into_iter().next().ok_or("Metal device unavailable")?;
        return Ok(("unified".into(), vec![EngineDevice { id: id.into(), name, memory_mib: memory, driver_version: driver.trim().into(), engine_device: selector }]));
    }
    let summary = observers.output(Observer::Tool("vulkaninfo"), &["--summary"]).await?;
    Ok(("dedicated".into(), vulkan_devices(&summary, observed)?))
}

#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for WindowsJob {}
#[cfg(windows)]
unsafe impl Sync for WindowsJob {}
#[cfg(windows)]
impl WindowsJob {
    fn attach(pid: u32) -> Result<Self, String> {
        use windows_sys::Win32::{Foundation::CloseHandle, System::{JobObjects::*, Threading::*}};
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() { return Err("cannot create engine containment job".into()); }
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(handle, JobObjectExtendedLimitInformation, &limits as *const _ as *const _, std::mem::size_of_val(&limits) as u32) == 0 { return Err("cannot protect engine containment job".into()); }
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() { return Err("cannot contain engine process".into()); }
            let assigned = AssignProcessToJobObject(handle, process);
            CloseHandle(process);
            if assigned == 0 { return Err("cannot assign engine containment job".into()); }
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for WindowsJob { fn drop(&mut self) { unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0); } } }

#[cfg(test)]
#[path = "engine_runtime_tests.rs"]
mod tests;
