#!/usr/bin/env python3
"""Qualify a managed native llama.cpp runtime and real prepaid gateway.

No downloads, funding or passing evidence are synthesized. Run on the runtime
host with private admin/engine credentials and funded infer+read bearer files.
Windows/Linux CUDA and macOS unified memory are measured using actual platform
counters; unsupported memory counters fail closed. This is a provider declaration,
not an independent execution attestation.
"""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import secrets
import stat
import subprocess
import platform
import re
import importlib
import threading
import time
import urllib.error
import urllib.request
import urllib.parse

MAX_BYTES = 64 * 1024 * 1024

def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()

def sha(data):
    return hashlib.sha256(data).hexdigest()

def load(path):
    data = Path(path).read_bytes()
    if len(data) > MAX_BYTES:
        raise ValueError("input exceeds 64 MiB")
    return json.loads(data)

def command(*args):
    result = subprocess.run(args, check=True, capture_output=True, text=True, timeout=30)
    if len(result.stdout) > MAX_BYTES:
        raise ValueError("inspection output exceeds limit")
    return result.stdout

def secret(path):
    metadata = os.stat(path, follow_symlinks=False)
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 4096:
        raise ValueError("bearer files must be private regular files <=4096 bytes")
    provision_provider().private_file(Path(path))
    value = Path(path).read_text().strip()
    if not value or any(c.isspace() for c in value):
        raise ValueError("invalid bearer file")
    return value

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise ValueError("qualification endpoints must not redirect")

OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

def request(base, path, body=None, token=None, key=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    if key:
        headers["X-Request-Id"] = key
        headers["Idempotency-Key"] = key
    payload = None if body is None else canonical(body)
    return OPENER.open(urllib.request.Request(base.rstrip("/") + path, payload, headers), timeout=120)

def get_json(base, path, token=None):
    with request(base, path, token=token) as response:
        data = response.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("response exceeds bound")
    return json.loads(data)

def provision_provider():
    return importlib.import_module("provision-provider")

OBSERVED_FIELDS = "platform backend memory_kind devices runtime_sha256 artifact_manifest_sha256 engine_version memory_reserve_mib gpu_layers split_mode tensor_split cache_type_k cache_type_v flash_attention batch_size ubatch_size max_concurrency".split()

def private_origin(value):
    origin = urllib.parse.urlsplit(value)
    if (origin.scheme != "http" or origin.hostname not in ("127.0.0.1", "::1")
            or origin.username or origin.password or origin.query or origin.fragment or origin.path not in ("", "/")):
        raise ValueError("private endpoints must use literal loopback HTTP origins")
    return origin

def inspect_runtime(args, model):
    profile = model["serving"]
    private_origin(args.admin_url)
    private_origin(args.engine_url)
    rows = get_json(args.admin_url, "/admin/runtime", secret(args.admin_key_file))["models"]
    matches = [entry for entry in rows if entry["model_id"] == model["id"]]
    # `alive` is the managed engine process's liveness, not readiness, which `/admin/runtime`
    # reports apart; the paid gateway samples of `main` fail unless the provider admits the model.
    if (len(matches) != 1 or matches[0].get("managed") is not True or matches[0].get("alive") is not True
            or not matches[0].get("observation")):
        raise ValueError("qualification requires one live managed runtime observation")
    observed = matches[0]["observation"]
    if observed["engine_url"].rstrip("/") != args.engine_url.rstrip("/") or observed["model_id"] != model["id"]:
        raise ValueError("engine URL/model differs from observed managed process")
    if type(observed["pid"]) is not int or observed["pid"] <= 0:
        raise ValueError("missing observed process identity")
    if profile["engine"] != "llama.cpp":
        raise ValueError("only pinned llama.cpp can qualify")
    for field in OBSERVED_FIELDS:
        if observed.get(field) != profile.get(field):
            raise ValueError("observed runtime differs from candidate: " + field)
    if observed["context_length"] != model["context_length"]:
        raise ValueError("observed context differs from model")
    host = {"Linux": "linux", "Windows": "windows", "Darwin": "macos"}.get(platform.system())
    if observed["platform"] != host:
        raise ValueError("qualification must run on the managed runtime host")
    root = Path(args.artifact_root).resolve(strict=True)
    base, task = provision_provider().validate_artifact(root)
    if base != dict(model, serving=None) or task != "generation":
        raise ValueError("selected generation model differs from verified artifact")
    manifest_bytes = (root / "chiral-artifact-sha256.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    if sha(manifest_bytes) != observed["artifact_manifest_sha256"]:
        raise ValueError("observed artifact manifest differs")
    if any(profile[field] != manifest["model"]["repo_id"] for field in ("hf_repo", "tokenizer_repo")):
        raise ValueError("observed embedded tokenizer/model provenance differs")
    devices = observed["devices"]
    if not devices or len({d["id"] for d in devices}) != len(devices):
        raise ValueError("missing or duplicated observed physical devices")
    if observed["memory_kind"] == "unified":
        if len(devices) != 1 or profile["memory_reserve_mib"] < 4096:
            raise ValueError("unified memory counts physical capacity once and requires 4096 MiB headroom")
        capacity_memory = devices[0]["memory_mib"]
    elif observed["memory_kind"] == "dedicated":
        if profile["memory_reserve_mib"] < 256:
            raise ValueError("dedicated memory requires headroom")
        capacity_memory = sum(d["memory_mib"] for d in devices) if profile["capacity_tier"] == "higher" else min(d["memory_mib"] for d in devices)
    else:
        raise ValueError("system/CPU memory cannot certify a paid speed tier")
    capacity = {"8gb": 7500, "16gb": 15000, "32gb": 30000, "higher": 45000}.get(profile["capacity_tier"])
    if capacity is None or capacity_memory < capacity:
        raise ValueError("observed hardware does not fit declared capacity tier")
    if profile["policy"] != "warm-generation-v1" or profile["output_tokens_per_second"] not in (20, 50, 100):
        raise ValueError("unsupported performance class/policy")
    if not 1 <= profile["max_concurrency"] <= 256 or not 1 <= profile["arrival_milli_rps"] <= 1000000:
        raise ValueError("invalid admission envelope")
    if model["context_length"] < 4096 or model["max_output_tokens"] < 256 or not any(cap in model["capabilities"] for cap in ("chat", "completions")) or "embeddings" in model["capabilities"]:
        raise ValueError("unsupported workload/capabilities")
    # Ensure actual memory counters exist before funded dispatch, not afterwards.
    memory_reading(observed)
    return observed

def probe_completion_echo(args, model, token):
    """Probe the paid public echo contract, not a private engine extension."""
    if "completions" not in model["capabilities"]:
        return
    prompt = "Chiral immutable completion echo protocol."
    key = "qualification-echo-" + secrets.token_hex(16)
    args.journal.append({"phase": "prepared", "key": key, "endpoint": "completions", "stream": False, "prompt_sha256": sha(prompt.encode())})
    with request(args.gateway_url, "/v1/completions",
                 {"model": model["id"], "prompt": prompt, "max_tokens": 1,
                  "temperature": 0, "echo": True, "logprobs": 0}, token, key) as response:
        value = json.loads(response.read(MAX_BYTES + 1))
        request_id = response.headers.get("X-Request-Id", key)
    logprobs = value["choices"][0].get("logprobs")
    generated = value["usage"]["completion_tokens"]
    # Log-probabilities cover the generated tokens only, and their text offsets start
    # after the echoed prompt (public-inference-api requirement 9).
    if (not value["choices"][0]["text"].startswith(prompt)
            or not isinstance(logprobs, dict)
            or generated != 1
            or any(not isinstance(logprobs.get(field), list) or len(logprobs[field]) != generated
                   for field in ("tokens", "token_logprobs", "top_logprobs", "text_offset"))
            or logprobs["text_offset"][0] != len(prompt)):
        raise ValueError("public completion echo/logprobs contract failed")
    view = get_json(args.gateway_url, "/v1/requests/" + request_id, token)
    if view["status"] != "completed" or view["usage"] != value["usage"]:
        # Public usage may also include total_tokens.
        if view["status"] != "completed" or any(view["usage"].get(k) != value["usage"].get(k) for k in ("prompt_tokens", "completion_tokens")):
            raise ValueError("echo accounting mismatch")
    args.journal.append({"phase": "completed", "key": key, "request_id": request_id})

def memory_reading(runtime):
    """Return metric, aggregate used MiB, minimum selected-device available MiB."""
    if runtime["backend"] == "cuda" and runtime["memory_kind"] == "dedicated":
        raw = command("nvidia-smi", "--query-gpu=uuid,memory.total,memory.used,memory.free", "--format=csv,noheader,nounits")
        rows = {}
        for line in raw.strip().splitlines():
            uuid, total, used, free = [part.strip() for part in line.split(",")]
            rows[uuid] = (int(total), int(used), int(free))
        selected = []
        for device in runtime["devices"]:
            if device["id"] not in rows or rows[device["id"]][0] != device["memory_mib"]:
                raise ValueError("memory counter physical inventory differs from runtime")
            selected.append(rows[device["id"]])
        return "device_used", sum(row[1] for row in selected), min(row[2] for row in selected)
    if runtime["platform"] == "macos" and runtime["backend"] == "metal" and runtime["memory_kind"] == "unified":
        total = int(command("sysctl", "-n", "hw.memsize")) // (1024 * 1024)
        if len(runtime["devices"]) != 1 or runtime["devices"][0]["memory_mib"] != total:
            raise ValueError("unified memory capacity differs from observed host")
        raw = command("vm_stat")
        page = re.search(r"page size of ([0-9]+) bytes", raw)
        values = dict(re.findall(r"^([^:]+):\s*([0-9]+)\.", raw, re.MULTILINE))
        if page is None or "Pages free" not in values:
            raise ValueError("unrecognized macOS physical memory counters")
        # Conservatively count inactive/cache/compressed pages as used. Never
        # relabel process RSS or an invented GPU allocation as dedicated VRAM.
        available = int(values["Pages free"]) * int(page[1]) // (1024 * 1024)
        return "system_used", total - available, available
    raise ValueError("no trustworthy memory counter for this runtime/backend; qualification refused")

class MemorySampler:
    def __init__(self, runtime):
        self.runtime = runtime
        self.stop = threading.Event()
        self.peak = 0
        self.minimum_available = None
        self.metric = None
        self.failure = None
        self.worker = threading.Thread(target=self.run, daemon=True)

    def run(self):
        try:
            while not self.stop.is_set():
                metric, used, available = memory_reading(self.runtime)
                if used < 0 or available < 0 or (self.metric and metric != self.metric):
                    raise ValueError("invalid/changing physical memory metric")
                self.metric = metric
                self.peak = max(self.peak, used)
                self.minimum_available = available if self.minimum_available is None else min(self.minimum_available, available)
                self.stop.wait(0.2)
        except Exception as error:
            self.failure = error

    def __enter__(self):
        self.worker.start()
        return self

    def __exit__(self, kind, value, trace):
        self.stop.set()
        self.worker.join(35)
        if self.failure is not None or self.worker.is_alive() or self.minimum_available is None:
            raise ValueError("runtime physical memory observation failed")

class RequestJournal:
    """Crash-safe paid dispatch keys only; never prompts, output, or credentials."""
    def __init__(self, path):
        self.lock = threading.Lock()
        self.fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        provision_provider().protect(Path(path), 0o600)

    def append(self, value):
        data = canonical(value) + b"\n"
        with self.lock:
            offset = 0
            while offset < len(data):
                written = os.write(self.fd, data[offset:])
                if written <= 0:
                    raise OSError("qualification request journal did not accept data")
                offset += written
            os.fsync(self.fd)

class Inflight:
    def __init__(self):
        self.lock = threading.Lock()
        self.current = 0
        self.peak = 0

    def enter(self):
        with self.lock:
            self.current += 1
            self.peak = max(self.peak, self.current)

    def leave(self):
        with self.lock:
            self.current -= 1

def workload_prompt(args, model, endpoint, target, engine_token):
    """Prepare exact-token-length unique workloads before the admission clock."""
    prefix = "Unique experiment " + secrets.token_hex(16) + ". "
    suffix = "\nWrite an extensive technical essay of at least 1500 words about distributed systems. Continue with detailed examples and do not finish early."
    low, high = 0, target * 4
    best = None
    for _ in range(16):
        if low > high:
            break
        length = (low + high) // 2
        prompt = prefix + (" a" * length) + suffix
        content = prompt
        if endpoint == "chat":
            with request(args.engine_url, "/apply-template",
                         {"messages": [{"role": "user", "content": prompt}],
                          "add_generation_prompt": True, "continue_final_message": False,
                          "chat_template_kwargs": {"enable_thinking": False}}, engine_token) as response:
                raw = response.read(MAX_BYTES + 1)
            if len(raw) > MAX_BYTES:
                raise ValueError("template response exceeds limit")
            content = json.loads(raw)["prompt"]
        with request(args.engine_url, "/tokenize",
                     {"content": content, "add_special": True, "parse_special": True}, engine_token) as response:
            raw = response.read(MAX_BYTES + 1)
        if len(raw) > MAX_BYTES:
            raise ValueError("tokenization response exceeds limit")
        ids = json.loads(raw)["tokens"]
        if not isinstance(ids, list) or not ids or any(type(x) is not int or x < 0 for x in ids):
            raise ValueError("engine tokenizer lacks authoritative input IDs")
        count = len(ids)
        if count <= target:
            best = (prompt, count, ids)
            low = length + 1
        else:
            high = length - 1
    if best is None or not target - 8 <= best[1] <= target:
        raise ValueError("cannot construct requested workload token boundary with pinned tokenizer")
    return best

def sample(args, model, path, endpoint, streaming, index, token, tracker, prepared=None):
    prompt, expected_tokens, input_ids = prepared or workload_prompt(args, model, endpoint, 128,
        secret(args.engine_key_file))
    body = {"model": model["id"], "max_tokens": 256, "temperature": 0.7, "stream": streaming}
    if endpoint == "chat":
        body["messages"] = [{"role": "user", "content": prompt}]
        route = "/v1/chat/completions"
    else:
        body["prompt"] = prompt
        route = "/v1/completions"
    if streaming:
        body["stream_options"] = {"include_usage": True}
    if path == "engine":
        body.update(return_tokens=True, cache_prompt=False, n=1)
        if endpoint == "completions":
            body.update(prompt=input_ids, verbose=True, n_probs=0,
                        response_fields=["tokens", "truncated"])
        else:
            body.update(n_probs=1, add_generation_prompt=True, continue_final_message=False,
                        chat_template_kwargs={"enable_thinking": False})
    key = "qualification-" + secrets.token_hex(16)
    if path == "gateway":
        args.journal.append({"phase": "prepared", "key": key, "endpoint": endpoint, "stream": streaming, "prompt_sha256": sha(prompt.encode())})
    start = time.monotonic_ns()
    first = last = None
    usage = None
    observed_tokens = 0
    token_clock_valid = True
    result = {"prompt_tokens": expected_tokens, "output_tokens": 0, "ttft_us": 0, "decode_us": 0, "elapsed_us": 0, "error": True, "usage_known": False,
              "endpoint": endpoint, "client_stream": streaming, "prompt_sha256": sha(prompt.encode()),
              "external_ttft_us": None, "external_decode_us": None, "external_elapsed_us": 0}
    tracker.enter()
    active = True
    try:
        base = args.engine_url if path == "engine" else args.gateway_url
        with request(base, route, body, token=token, key=key) as response:
            request_id = response.headers.get("X-Request-Id", key)
            if streaming:
                total = 0
                done = False
                for line in response:
                    total += len(line)
                    if total > MAX_BYTES:
                        raise ValueError("stream exceeds bound")
                    if not line.startswith(b"data:"):
                        continue
                    data = line[5:].strip()
                    if data == b"[DONE]":
                        done = True
                        break
                    value = json.loads(data)
                    if "error" in value:
                        raise ValueError("inference stream error")
                    for choice in value.get("choices", []):
                        delta = choice.get("delta", {})
                        visible_output = bool(choice.get("text") or delta.get("content") or delta.get("tool_calls"))
                        ids = None
                        if path == "engine":
                            if endpoint == "chat":
                                logs = (choice.get("logprobs") or {}).get("content")
                                ids = [item["id"] for item in logs] if isinstance(logs, list) else None
                            else:
                                ids = value.get("__verbose", {}).get("tokens")
                        if path == "engine":
                            if ids is None:
                                token_clock_valid &= not visible_output
                            else:
                                if not isinstance(ids, list):
                                    raise ValueError("engine token_ids is not an authoritative token array")
                                token_clock_valid &= len(ids) <= 1 and all(type(item) is int and item >= 0 for item in ids) and (bool(ids) or not visible_output)
                                observed_tokens += len(ids)
                        if ids or visible_output:
                            now = time.monotonic_ns()
                            first = now if first is None else first
                            last = now
                    if value.get("usage"):
                        usage = value["usage"]
                if not done:
                    raise ValueError("stream ended without a terminal marker")
            else:
                data = response.read(MAX_BYTES + 1)
                if len(data) > MAX_BYTES:
                    raise ValueError("response exceeds bound")
                usage = json.loads(data)["usage"]
        tracker.leave()
        active = False
        if path == "engine" and (not token_clock_valid or not isinstance(usage, dict) or observed_tokens != usage.get("completion_tokens")):
            raise ValueError("engine lacks complete single-token observation boundaries")
        elapsed = (time.monotonic_ns() - start) // 1000
        result["external_elapsed_us"] = elapsed
        if path == "gateway":
            args.journal.append({"phase": "completed", "key": key, "request_id": request_id})
        if not isinstance(usage, dict) or not isinstance(usage.get("completion_tokens"), int):
            raise ValueError("missing authoritative engine usage")
        result.update(prompt_tokens=usage["prompt_tokens"], output_tokens=usage["completion_tokens"], elapsed_us=elapsed)
        result["usage_known"] = True
        if usage["prompt_tokens"] != expected_tokens:
            raise ValueError("final engine usage differs from pinned tokenizer workload")
        if first is not None:
            result["external_ttft_us"] = (first - start) // 1000
            result["external_decode_us"] = (last - first) // 1000
        if path == "gateway":
            view = get_json(base, "/v1/requests/" + request_id, token)
            measured = view.get("performance") or {}
            if view["status"] != "completed" or view["usage"] != {"prompt_tokens": usage["prompt_tokens"], "completion_tokens": usage["completion_tokens"]}:
                raise ValueError("gateway final accounting mismatch")
            result.update(ttft_us=measured["ttft_us"], decode_us=measured["decode_us"], elapsed_us=measured["elapsed_us"])
            account = get_json(base, "/v1/contracts/" + view["contract_id"], token)
            if int(account["funded_wei"]) <= 0 or (account["price"]["input_wei_per_1k"] == "0" and account["price"]["output_wei_per_1k"] == "0"):
                raise ValueError("gateway qualification requires genuinely funded paid inference")
            expected_model = dict(model)
            expected_model["serving"] = None
            if account["model"] != expected_model or account["max_concurrency"] != model["serving"]["max_concurrency"]:
                raise ValueError("calibration gateway agreement differs from inspected model or concurrency")
        else:
            result.update(ttft_us=result["external_ttft_us"], decode_us=result["external_decode_us"])
        result["error"] = any(result[k] is None for k in ("ttft_us", "decode_us", "elapsed_us"))
    except (ValueError, KeyError, OSError, urllib.error.URLError):
        # No prompt, bearer, response body or remote error text is persisted.
        result["elapsed_us"] = (time.monotonic_ns() - start) // 1000
        result["external_elapsed_us"] = result["elapsed_us"]
    finally:
        if active:
            tracker.leave()
    for field in ("ttft_us", "decode_us", "elapsed_us"):
        if result[field] is None:
            result[field] = 0
    return result

def passes(sample, rate):
    return (not sample["error"] and sample["usage_known"] and 0 < sample["prompt_tokens"] <= 2048 and sample["output_tokens"] == 256
            and 0 < sample["ttft_us"] <= 2000000 and 0 < sample["decode_us"]
            and sample["decode_us"] * rate <= 255000000
            and sample["elapsed_us"] >= sample["ttft_us"] + sample["decode_us"])

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-model", required=True, help="ModelSpec with complete declared serving profile; qualification is replaced only after measurement")
    parser.add_argument("--artifact-root", required=True)
    parser.add_argument("--admin-url", required=True, help="private literal loopback admin origin")
    parser.add_argument("--admin-key-file", required=True)
    parser.add_argument("--engine-url", required=True)
    parser.add_argument("--gateway-url", required=True)
    parser.add_argument("--engine-key-file", required=True)
    parser.add_argument("--gateway-key-file", action="append", required=True, help="private infer+read key file; repeat with distinct funded accounts for >120 requests/minute")
    parser.add_argument("--probe-location", required=True)
    parser.add_argument("--evidence-out", required=True)
    parser.add_argument("--model-out", required=True)
    parser.add_argument("--samples-per-mode", type=int, default=100)
    args = parser.parse_args()
    if not 100 <= args.samples_per_mode <= 1000:
        parser.error("samples-per-mode must be between 100 and 1000")
    if Path(args.evidence_out).exists() or Path(args.model_out).exists():
        parser.error("refusing to overwrite previous evidence or selected model")
    # Reusing a prior run's output identity is refused before any funded dispatch.
    args.journal = RequestJournal(args.evidence_out + ".requests.jsonl")
    model = load(args.candidate_model)
    profile = model["serving"]
    runtime = inspect_runtime(args, model)
    tokens = [secret(path) for path in args.gateway_key_file]
    engine_token = secret(args.engine_key_file) if args.engine_key_file else None
    probe_completion_echo(args, model, tokens[0])
    identity_model = json.loads(json.dumps(model))
    identity_model["serving"].pop("qualification", None)
    identity = sha(canonical(identity_model))
    report = {"schema": "chiral-performance-evidence-v2", "profile_sha256": identity,
              "runtime_identity_sha256": sha(canonical(runtime)), "runtime_observation": runtime, "measured_at": int(time.time()),
              "probe_location": args.probe_location, "transport": "HTTP/1.1 urllib no proxy; gateway and external clocks separate",
              "sweeps": []}
    endpoints = [name for name in ("chat", "completions") if name in model["capabilities"]]
    success = True
    for path in ("engine", "gateway"):
        modes = [(endpoint, streaming) for endpoint in endpoints for streaming in ([True] if path == "engine" else [True, False])]
        # Real warmup is discarded and uses unique prompts, not benchmark cache hits.
        for endpoint, streaming in modes:
            warm = sample(args, model, path, endpoint, streaming, 0, engine_token if path == "engine" else tokens[0], Inflight())
            if warm["error"]:
                raise ValueError("warmup or paid gateway accounting failed; no qualification emitted")
        for concurrency in range(1, profile["max_concurrency"] + 1):
            # Bursts exercise actual advertised overlap, with sustained rate and
            # burst size identical to the gateway's token-bucket envelope.
            count = max(len(modes) * args.samples_per_mode, concurrency)
            count = ((count + concurrency - 1) // concurrency) * concurrency
            tasks = []
            for index in range(count):
                endpoint, streaming = modes[index % len(modes)]
                target = [128, 512, 1024, 2048][(index // len(modes)) % 4]
                prompt, prompt_tokens, input_ids = workload_prompt(args, model, endpoint, target, engine_token)
                tasks.append((endpoint, streaming, prompt, prompt_tokens, input_ids))
            tracker = Inflight()
            rate = profile["arrival_milli_rps"] / 1000
            start = time.monotonic()
            results = []
            futures = set()
            with MemorySampler(runtime) as memory, concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as pool:
                for index, (endpoint, streaming, prompt, prompt_tokens, input_ids) in enumerate(tasks):
                    planned = start + (index // concurrency) * concurrency / rate
                    time.sleep(max(0, planned - time.monotonic()))
                    finished = {future for future in futures if future.done()}
                    results.extend(future.result() for future in finished)
                    futures -= finished
                    if len(futures) >= concurrency:
                        # Offered load is not silently queued into a lower arrival envelope.
                        results.append({"prompt_tokens": prompt_tokens, "output_tokens": 0, "ttft_us": 0, "decode_us": 0, "elapsed_us": 0, "error": True, "usage_known": False,
                                        "endpoint": endpoint, "client_stream": streaming, "prompt_sha256": sha(prompt.encode()),
                                        "external_ttft_us": None, "external_decode_us": None, "external_elapsed_us": 0})
                    else:
                        token = engine_token if path == "engine" else tokens[index % len(tokens)]
                        futures.add(pool.submit(sample, args, model, path, endpoint, streaming, index, token, tracker, (prompt, prompt_tokens, input_ids)))
                time.sleep(max(0, start + len(tasks) / rate - time.monotonic()))
                offered_duration_ms = max(1, int((time.monotonic() - start) * 1000))
                results.extend(future.result() for future in futures)
            passed = sum(passes(item, profile["output_tokens_per_second"]) for item in results)
            per_mode = all(sum(passes(item, profile["output_tokens_per_second"]) for item in results if (item["endpoint"], item["client_stream"]) == mode) * 100 >= sum((item["endpoint"], item["client_stream"]) == mode for item in results) * 95 for mode in modes)
            for mode in modes:
                for target in (128, 512, 1024, 2048):
                    bucket = [item for item in results if (item["endpoint"], item["client_stream"]) == mode and target - 8 <= item["prompt_tokens"] <= target]
                    per_mode &= len(bucket) >= 20 and sum(passes(item, profile["output_tokens_per_second"]) for item in bucket) * 100 >= len(bucket) * 95
            rate_exercised = len(results) * 1000000 * 100 >= profile["arrival_milli_rps"] * offered_duration_ms * 95 and len(results) * 1000000 * 100 <= profile["arrival_milli_rps"] * offered_duration_ms * 105
            success &= passed * 100 >= len(results) * 95 and per_mode and rate_exercised and tracker.peak == concurrency
            reserve = profile["memory_reserve_mib"]
            total_memory = sum(device["memory_mib"] for device in runtime["devices"])
            reserved_memory = reserve * (len(runtime["devices"]) if runtime["memory_kind"] == "dedicated" else 1)
            success &= memory.minimum_available >= reserve and memory.peak <= total_memory - reserved_memory
            report["sweeps"].append({"path": path, "concurrency": concurrency, "observed_concurrency": tracker.peak,
                "arrival_milli_rps": profile["arrival_milli_rps"], "duration_ms": offered_duration_ms,
                "peak_memory_mib": memory.peak, "min_available_memory_mib": memory.minimum_available,
                "memory_metric": memory.metric, "requests": results})
            print(json.dumps({"path": path, "concurrency": concurrency, "joint_passes": passed, "requests": len(results), "passed": per_mode}), flush=True)
    # Reinspect after the run so a redeploy/settings change invalidates the run.
    if inspect_runtime(args, model) != runtime:
        raise ValueError("deployment identity changed during qualification")
    evidence = canonical(report) + b"\n"
    if len(evidence) > MAX_BYTES:
        raise ValueError("evidence exceeds bounded Rust report size; no qualified profile emitted")
    Path(args.evidence_out).write_bytes(evidence)
    if not success:
        raise SystemExit("Requested class failed; evidence retained, no qualified ModelSpec written")
    profile["qualification"] = {"evidence_sha256": sha(evidence), "measured_at": report["measured_at"]}
    Path(args.model_out).write_bytes(canonical(model) + b"\n")
    print("Measured evidence and selected ModelSpec written; reconfigure and pass Rust readiness before publication.")

if __name__ == "__main__":
    main()
