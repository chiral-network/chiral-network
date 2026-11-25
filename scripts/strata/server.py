#!/usr/bin/env python3
"""Private Chiral adapter for the pinned Strata Inkling native line protocol.

Only text chat is exposed. No upstream HTTP management UI, automatic retries,
model downloads, tools, fallback, context fitting or output retokenization.
"""
import argparse
import codecs
import ctypes
import importlib
from importlib.metadata import version
import hmac
import ipaddress
import json
import math
import os
from pathlib import Path
import queue
import select
import signal
import socket
import subprocess
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# -I intentionally excludes the script directory from the import search path.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import ENGINE_VERSION, SOURCE_REVISION, entry, model_spec, read_json, require, host_available_mib

MAX_BODY = 256 * 1024
MAX_LINE = 65536


def verify_python(runtime):
    root = (runtime / 'python').resolve()
    require(sys.version_info >= (3, 12) and Path(sys.executable).resolve().is_relative_to(root)
            and Path(sys.prefix).resolve().is_relative_to(root)
            and Path(sys.base_prefix).resolve().is_relative_to(root), 'Python must be self-contained in the runtime')
    for name, expected in {'regex': '2026.9.10', 'jinja2': '3.1.6', 'markupsafe': '3.0.3'}.items():
        module = importlib.import_module(name)
        require(version(name) == expected and Path(module.__file__).resolve().is_relative_to(root),
                'Python dependency differs from pinned runtime')


def parent_death():
    if sys.platform.startswith('linux'):
        parent = os.getppid()
        if ctypes.CDLL(None).prctl(1, signal.SIGKILL, 0, 0, 0) != 0 or os.getppid() != parent:
            os._exit(1)


class NativeEngine:
    def __init__(self, argv, context, backend, timeout=180, env=None):
        self.lines = queue.Queue(maxsize=1024)
        self.lock = threading.Lock()
        self.info = {}
        self.proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, env=env,
                                     preexec_fn=parent_death if sys.platform.startswith('linux') else None)
        self.reader = threading.Thread(target=self._pump, daemon=True)
        self.reader.start()
        try:
            deadline = time.monotonic() + timeout
            for _ in range(1024):
                line = self.read(deadline)
                if line.startswith('INFO '):
                    self.info.update(x.split('=', 1) for x in line.split()[1:] if '=' in x)
                elif line.startswith('READY '):
                    parts = line.split()
                    require(int(parts[1]) == context and 'stop' in parts[2:], 'Strata context/cancellation mismatch')
                    require(self.info.get('engine') == 'strata-inkling-0.1.69'
                            and self.info.get('arch') == 'inkling2_loop'
                            and self.info.get('backend') == backend
                            and self.info.get('batch_slots') == '1'
                            and self.info.get('kv_dtype') == 'fp16'
                            and self.info.get('drafting') == 'dspark', 'Strata backend/architecture/slots/recipe mismatch')
                    return
                elif line.startswith('ERR'):
                    raise ValueError('Strata failed during startup')
            raise ValueError('Strata startup exceeded protocol limit')
        except BaseException:
            self.close()
            raise

    def _pump(self):
        try:
            while True:
                line = self.proc.stdout.readline(MAX_LINE + 1)
                if not line or len(line) > MAX_LINE or not line.endswith(b'\n'):
                    break
                self.lines.put(line.decode('utf-8', errors='strict').strip())
        except (OSError, UnicodeError):
            pass
        finally:
            self.lines.put(None)

    def alive(self):
        return self.proc.poll() is None

    def read(self, deadline, cancel=None):
        while time.monotonic() < deadline:
            if cancel is not None and cancel.is_set():
                raise InterruptedError('request cancelled')
            try:
                line = self.lines.get(timeout=min(0.1, max(0.001, deadline - time.monotonic())))
            except queue.Empty:
                continue
            require(line is not None, 'Strata exited without terminal usage')
            return line
        raise TimeoutError('Strata protocol deadline exceeded')

    def send(self, line):
        require(self.alive(), 'Strata is unavailable')
        self.proc.stdin.write((line + '\n').encode())
        self.proc.stdin.flush()

    def generate(self, ids, maximum, sampling, cancel, timeout):
        require(self.lock.acquire(blocking=False), 'Strata is busy')
        done = False
        try:
            require(self.alive() and self.lines.empty(), 'Strata protocol is out of step')
            keys = ''.join(' ' + name + '=' + str(value) for name, value in sampling.items())
            self.send(f'GEN {maximum}{keys} ' + ','.join(map(str, ids)))
            deadline = time.monotonic() + timeout
            count = 0
            while True:
                line = self.read(deadline, cancel)
                if line.startswith('T '):
                    token = int(line[2:])
                    require(0 <= token <= 2147483647 and count < maximum, 'invalid/excess Strata output token')
                    count += 1
                    yield ('token', token)
                elif line.startswith('DONE '):
                    f = line.split()
                    require(len(f) >= 6 and int(f[1]) == count and int(f[2]) == len(ids)
                            and f[5] in ('stop', 'length', 'eos'), 'Strata terminal usage mismatch')
                    require(count <= maximum, 'Strata exceeded output limit')
                    done = True
                    yield ('done', {'prompt_tokens': len(ids), 'completion_tokens': count,
                                    'total_tokens': len(ids) + count,
                                    'finish_reason': 'length' if f[5] == 'length' else 'stop'})
                    return
                elif line.startswith(('PP ', 'RESUME ', 'INFO ')):
                    continue
                else:
                    raise ValueError('unexpected Strata generation response')
        finally:
            if not done:
                # A cancelled/failed exchange is never billed. Drain the same child
                # before releasing its slot; a failed drain permanently stops it.
                try:
                    self.send('STOP')
                    deadline = time.monotonic() + 5
                    while not self.read(deadline).startswith('DONE '):
                        pass
                except (OSError, ValueError, TimeoutError):
                    self.close()
            self.lock.release()

    def close(self):
        if self.alive():
            self.proc.kill()
        self.proc.wait(timeout=5)
        self.reader.join(timeout=0.2)
        self.proc.stdin.close()
        self.proc.stdout.close()


class TextModel:
    def __init__(self, runtime, artifacts):
        sys.path.insert(0, str(runtime / 'upstream/terminal'))
        sys.path.insert(0, str(runtime / 'upstream/terminal/tools'))
        from strata_tokenizer import Tokenizer
        from serve import frontend, inkling_dialect
        self.frontend, self.dialect = frontend, inkling_dialect
        data = read_json(artifacts / 'tokenizer.json', 32 * 1024 * 1024)
        vocab, added = data['model']['vocab'], data['added_tokens']
        require(data['model']['type'] == 'BPE', 'unexpected catalog tokenizer')
        size = max(max(vocab.values()), max(a['id'] for a in added)) + 1
        require(size == 200058, 'unexpected Inkling vocabulary size')
        tokens, types = [None] * size, [1] * size
        for text, idx in vocab.items():
            tokens[idx] = text
        for a in added:
            require(tokens[a['id']] in (None, a['content']), 'conflicting tokenizer id')
            tokens[a['id']] = a['content']
            types[a['id']] = 3 if a['special'] else 4
        tokens = [t if t is not None else f'<|unused_{i}|>' for i, t in enumerate(tokens)]
        merges = [m if isinstance(m, str) else ' '.join(m) for m in data['model']['merges']]
        self.tok = Tokenizer(tokens, merges, types, 'o200k', {'bos': 200028, 'eos': 200006, 'pad': 199999})
        self.template = frontend.ChatTemplate(artifacts / 'chat_template.jinja')
        self.tags = frontend.literal_tags(self.tok.control_tokens)

    def prepare(self, request, contract):
        allowed = {'model', 'messages', 'max_tokens', 'stream', 'stream_options', 'n', 'temperature', 'top_p', 'seed'}
        require(isinstance(request, dict) and set(request) <= allowed, 'unsupported Strata launch parameter')
        require(request.get('model') == contract['id'] and request.get('n', 1) == 1, 'model/sequence mismatch')
        require(type(request.get('stream', False)) is bool
                and request.get('stream_options', {'include_usage': True}) == {'include_usage': True}, 'unsupported stream options')
        messages = request.get('messages')
        require(isinstance(messages, list) and 0 < len(messages) <= 256, 'invalid messages')
        for m in messages:
            require(isinstance(m, dict) and set(m) <= {'role', 'content'}
                    and m.get('role') in ('system', 'user', 'assistant')
                    and isinstance(m.get('content'), str), 'only text chat is supported')
        maximum = request.get('max_tokens', 256)
        require(type(maximum) is int and 1 <= maximum <= contract['max_output_tokens'], 'invalid output limit')
        sampling = {'temperature': 1.0, 'top_p': 1.0}
        for name, low, high in (('temperature', 0, 2), ('top_p', 0, 1)):
            if name in request:
                v = request[name]
                require(type(v) in (int, float) and math.isfinite(v) and low <= v <= high, 'invalid sampling')
                sampling[name] = v
        if 'seed' in request:
            require(type(request['seed']) is int and 0 <= request['seed'] <= 2147483647, 'invalid seed')
            sampling['seed'] = request['seed']
        marked, _, changed = self.frontend.mark_think_literals(messages, None, self.tags)
        prompt = self.dialect.finish_prompt(self.template.render(marked, enable_thinking=False, reasoning_effort='none'))
        plain = ()
        if changed:
            prompt, plain = self.frontend.unmark_think_literals(prompt, self.tags)
        ids = self.tok.encode(prompt, parse_special=True, plain=plain)
        require(ids and len(ids) + maximum <= contract['context_length'], 'context_length_exceeded')
        return ids, maximum, sampling


def handler(engine, text_model, contract, key, metadata, timeout):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.0'
        timeout = 10

        def log_message(self, *_):
            pass

        def authorized(self):
            if self.headers.get('Origin') or not hmac.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + key):
                self.reply(401, {'error': 'private engine authentication required'})
                return False
            return True

        def reply(self, status, value, error_code=None):
            data = json.dumps(value, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            if error_code is not None:
                self.send_header('X-Chiral-Error-Code', error_code)
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if not self.authorized():
                return
            if self.path == '/health':
                self.reply(200 if engine.alive() else 503, {'status': 'ok' if engine.alive() else 'error'})
            elif self.path == '/v1/models':
                self.reply(200, {'object': 'list', 'data': [{'id': contract['id'], 'object': 'model'}]})
            elif self.path == '/chiral/props':
                self.reply(200, metadata)
            else:
                self.reply(404, {'error': 'unknown private route'})

        def do_POST(self):
            if not self.authorized():
                return
            if self.path not in ('/chiral/prepare', '/v1/chat/completions'):
                self.reply(404, {'error': 'unknown private route'})
                return
            try:
                require(not self.headers.get('Transfer-Encoding'), 'chunked bodies are unsupported')
                require(len(self.headers.get_all('Content-Length', [])) == 1, 'ambiguous body length')
                length = int(self.headers.get('Content-Length', '0'))
                require(0 < length <= MAX_BODY, 'body exceeds limit')
                data = self.rfile.read(length)
                require(len(data) == length, 'truncated body')
                req = json.loads(data)
                ids, maximum, sampling = text_model.prepare(req, contract)
            except (ValueError, KeyError, TypeError) as error:
                code = 'context_length_exceeded' if str(error) == 'context_length_exceeded' else 'invalid_request'
                self.reply(400, {'error': 'unsupported request or context limit exceeded'}, code)
                return
            if self.path == '/chiral/prepare':
                self.reply(200, {'tokens': ids})
                return
            if not engine.alive() or engine.lock.locked():
                self.reply(503, {'error': 'Strata is unavailable or busy'})
                return
            cancel, finished = threading.Event(), threading.Event()

            def disconnect():
                while not finished.wait(0.05):
                    try:
                        readable, _, _ = select.select([self.connection], [], [], 0)
                        if readable and self.connection.recv(1, socket.MSG_PEEK) == b'':
                            cancel.set()
                            return
                    except OSError:
                        cancel.set()
                        return

            threading.Thread(target=disconnect, daemon=True).start()
            streamed = req.get('stream', False)
            base = {'id': 'chatcmpl-' + uuid.uuid4().hex, 'created': int(time.time()), 'model': contract['id'],
                    'object': 'chat.completion.chunk' if streamed else 'chat.completion'}
            decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
            dialect = text_model.dialect.InklingDialect()
            # The prepared prompt opens the content channel explicitly.
            dialect.feed_channels('<|message_model|><|content_text|>')
            output = []
            observed = 0
            iterator = engine.generate(ids, maximum, sampling, cancel, timeout)
            try:
                if streamed:
                    self.send_response(200)
                    self.send_header('Content-Type', 'text/event-stream')
                    self.end_headers()

                def event(value):
                    self.wfile.write(b'data: ' + json.dumps(value, ensure_ascii=False).encode() + b'\n\n')
                    self.wfile.flush()

                for kind, value in iterator:
                    if kind == 'token':
                        require(value < len(text_model.tok.tokens), 'output id outside vocabulary')
                        observed += 1
                        special = text_model.tok.token_types[value] == 3
                        raw = text_model.tok.token_bytes(value)
                        if special:
                            require(value not in (200005, 200008, 200049, 200057), 'unadvertised output channel')
                            pending = decoder.decode(b'', final=True)
                            decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
                            chunks = dialect.feed_token(pending, is_special=False) if pending else []
                            chunks += dialect.feed_token(raw.decode('utf-8'), is_special=True)
                        else:
                            # Flush an incomplete final UTF-8 sequence on its accepted
                            # token event, never as unmetered text after the token clock.
                            piece = decoder.decode(raw, final=observed == maximum)
                            chunks = dialect.feed_token(piece, is_special=False)
                        require(all(channel == 'content' for channel, _ in chunks), 'unexpected reasoning channel')
                        content = ''.join(t for _, t in chunks)
                        output.append(content)
                        if streamed:
                            event(dict(base, choices=[{'index': 0, 'delta': {'content': content}, 'finish_reason': None}],
                                       chiral_token_ids=[value]))
                    else:
                        tail = decoder.decode(b'', final=True)
                        chunks = dialect.feed_channels(tail, final=True, reason=value['finish_reason'])
                        require(all(channel == 'content' for channel, _ in chunks), 'unexpected reasoning channel')
                        tail = ''.join(t for _, t in chunks)
                        require(not tail, 'unobserved trailing output')
                        output.append(tail)
                        finish = value.pop('finish_reason')
                        if streamed:
                            event(dict(base, choices=[{'index': 0, 'delta': {'content': tail}, 'finish_reason': finish}], chiral_token_ids=[]))
                            event(dict(base, choices=[], usage=value))
                            self.wfile.write(b'data: [DONE]\n\n')
                        else:
                            self.reply(200, dict(base, choices=[{'index': 0, 'message': {'role': 'assistant', 'content': ''.join(output)},
                                                                'finish_reason': finish}], usage=value))
            except (ValueError, OSError, TimeoutError):
                if not streamed:
                    self.reply(502, {'error': 'Strata failed; no authoritative usage'})
                # A stream without a terminal checkpoint cannot settle a charge.
            finally:
                cancel.set()
                iterator.close()
                finished.set()
    return Handler


class BoundedServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, *args):
        self.readers = threading.BoundedSemaphore(8)
        super().__init__(*args)

    def process_request(self, request, client_address):
        if not self.readers.acquire(blocking=False):
            self.shutdown_request(request)
            return
        super().process_request(request, client_address)

    def process_request_thread(self, *args):
        try:
            super().process_request_thread(*args)
        finally:
            self.readers.release()


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    for name in ('runtime-dir', 'artifact-dir', 'catalog-id', 'host', 'port', 'api-key-file', 'backend', 'device', 'memory-reserve-mib', 'startup-timeout', 'request-timeout'):
        ap.add_argument('--' + name, required=True)
    a = ap.parse_args()
    require(ipaddress.ip_address(a.host).is_loopback(), 'engine must bind loopback')
    runtime, artifacts = Path(a.runtime_dir), Path(a.artifact_dir)
    verify_python(runtime)
    require(host_available_mib() >= 4096, 'Strata requires 4096 MiB available host RAM')
    e = entry(a.catalog_id, runtime / 'catalog.json')
    contract = model_spec(e)
    manifest = read_json(runtime / 'chiral-runtime.json')
    key = Path(a.api_key_file).read_text().strip()
    require(len(key) >= 32 and key.isascii() and not any(c.isspace() for c in key), 'invalid engine key')
    text_model = TextModel(runtime, artifacts)
    argv = [str(runtime / manifest['executable']), '--serve', '--model', str(artifacts / e['model_file']),
            '--max-context', str(e['context_length']), '--parallel', '1', '--draft', 'dspark',
            '--draft-beam', '1', '--kv', 'fp16', '--kv-offload', 'off', '--chunk', '512', '--no-anti-loop']
    if a.backend == 'cpu':
        argv += ['--cpu']
    else:
        argv += ['--gpu', a.device, '--vram-reserve-mib', a.memory_reserve_mib]
    native = NativeEngine(argv, e['context_length'], a.backend, int(a.startup_timeout), dict(os.environ))
    metadata = {'engine': 'strata', 'engine_version': ENGINE_VERSION, 'source_revision': SOURCE_REVISION,
                'catalog_id': e['id'], 'model_alias': contract['id'], 'context_length': e['context_length'],
                'max_concurrency': 1, 'chat_template_sha256': contract['chat_template_sha256'],
                'backend': a.backend, 'native_info': native.info}
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    server = None
    try:
        require(host_available_mib() >= 4096, 'Strata loading left insufficient host RAM')
        if ipaddress.ip_address(a.host).version == 6:
            BoundedServer.address_family = socket.AF_INET6
        server = BoundedServer((a.host, int(a.port)), handler(native, text_model, contract, key, metadata, int(a.request_timeout)))
        def watch_native():
            native.proc.wait()
            server.shutdown()
        threading.Thread(target=watch_native, daemon=True).start()
        server.serve_forever(poll_interval=0.1)
    finally:
        if server is not None:
            server.server_close()
        native.close()


if __name__ == '__main__':
    main()
