"""Protocol fixtures are not real-model or hardware qualification."""
import importlib
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent / 'strata'))
import common
import server

FIXTURE = r'''
import sys
mode = sys.argv[1]
backend = 'cuda' if mode == 'wrong-backend' else 'cpu'
print('INFO engine=strata-inkling-0.1.69 arch=inkling2_loop backend='+backend+' batch_slots=1 kv_dtype=fp16 drafting=dspark', flush=True)
print('READY 4096 stop hold holdb', flush=True)
for line in sys.stdin:
    if line.startswith('GEN '):
        count = len(line.strip().split()[-1].split(','))
        if mode == 'crash':
            sys.exit(1)
        print('T 42', flush=True)
        if mode != 'cancel':
            print('DONE '+('2' if mode == 'bad-usage' else '1')+' '+str(count)+' 1.0 2.0 length 3 8 0', flush=True)
    elif line.startswith('STOP'):
        print('DONE 1 2 1.0 2.0 cancel 0 0 0', flush=True)
'''


class NativeProtocol(unittest.TestCase):
    def engine(self, mode='ok'):
        engine = server.NativeEngine([sys.executable, '-u', '-c', FIXTURE, mode], 4096, 'cpu', timeout=2)
        self.addCleanup(engine.close)
        return engine

    def test_counts_only_observed_accepted_tokens_not_draft_proposals(self):
        engine = self.engine()
        events = list(engine.generate([10, 11], 5, {'temperature': 0}, threading.Event(), 2))
        self.assertEqual(events[0], ('token', 42))
        self.assertEqual(events[1], ('done', {'prompt_tokens': 2, 'completion_tokens': 1,
                                           'total_tokens': 3, 'finish_reason': 'length'}))

    def test_wrong_backend_refuses_startup_without_fallback(self):
        with self.assertRaisesRegex(ValueError, 'mismatch'):
            self.engine('wrong-backend')

    def test_terminal_usage_must_match_observed_tokens(self):
        engine = self.engine('bad-usage')
        emitted = []
        with self.assertRaisesRegex(ValueError, 'usage mismatch'):
            for event in engine.generate([10, 11], 5, {}, threading.Event(), 2):
                emitted.append(event)
        self.assertEqual(emitted, [('token', 42)])
        self.assertFalse(engine.lock.locked())

    def test_crash_never_produces_authoritative_usage_or_restart(self):
        engine = self.engine('crash')
        pid = engine.proc.pid
        with self.assertRaises(ValueError):
            list(engine.generate([10, 11], 5, {}, threading.Event(), 2))
        self.assertFalse(engine.alive())
        self.assertEqual(engine.proc.pid, pid)

    def test_cancellation_drains_before_reusing_the_slot(self):
        engine = self.engine('cancel')
        cancel = threading.Event()
        output = engine.generate([10, 11], 5, {}, cancel, 2)
        self.assertEqual(next(output), ('token', 42))
        other = engine.generate([10, 11], 5, {}, threading.Event(), 2)
        with self.assertRaisesRegex(ValueError, 'busy'):
            next(other)
        cancel.set()
        with self.assertRaises(InterruptedError):
            next(output)
        self.assertFalse(engine.lock.locked())
        self.assertTrue(engine.alive())
        again = engine.generate([10, 11], 5, {}, threading.Event(), 2)
        self.assertEqual(next(again), ('token', 42))
        again.close()


class CatalogContract(unittest.TestCase):
    def test_exact_allowlist_and_no_invented_qualification(self):
        entries = common.catalog()['entries']
        self.assertEqual([e['id'] for e in entries], ['inkling-2-mini-sq-10', 'inkling-2-mini-sq-15'])
        for e in entries:
            self.assertEqual(e['qualification'], 'unqualified')
            self.assertEqual(e['capabilities'], ['chat', 'streaming'])
            self.assertEqual(len(e['revision']), 40)
            self.assertEqual(common.model_spec(e)['serving'], None)
        with self.assertRaises(ValueError):
            common.entry('latest')

    def test_inventory_detects_tampering_extras_and_symlinks(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            item = root / 'model'; item.write_bytes(b'verified')
            files = {'model': {'size_bytes': 8, 'sha256': common.digest(item)}}
            common.inventory(root, files)
            item.write_bytes(b'tampered')
            with self.assertRaises(ValueError): common.inventory(root, files)
            item.write_bytes(b'verified')
            extra = root / 'extra'; extra.write_bytes(b'')
            with self.assertRaises(ValueError): common.inventory(root, files)
            extra.unlink(); extra.symlink_to(item)
            with self.assertRaises(ValueError): common.inventory(root, files)

    def test_provider_selects_fixed_strata_settings_explicitly(self):
        p = importlib.import_module('provision-provider')
        c = dict(strata_catalog='inkling-2-mini-sq-10', backend='cuda', device_ids=['CUDA0'],
                 gpu_layers=0, split_mode='none', tensor_split=[], cache_type_k='f16', cache_type_v='f16',
                 flash_attention=False, batch_size=512, ubatch_size=512, memory_reserve_mib=256, startup_timeout_secs=180)
        self.assertEqual(p.engine_settings(c, '/runtime', '/model')['strata_catalog'], c['strata_catalog'])
        with self.assertRaises(ValueError): p.engine_settings(dict(c, strata_catalog='latest'), '/runtime', '/model')
        with self.assertRaises(ValueError): p.engine_settings(dict(c, gpu_layers=1), '/runtime', '/model')


class PrivateHTTP(unittest.TestCase):
    def setUp(self):
        self.engine = server.NativeEngine([sys.executable, '-u', '-c', FIXTURE, 'ok'], 4096, 'cpu', timeout=2)
        class Tokenizer:
            tokens = [''] * 43
            token_types = [1] * 43
            def token_bytes(self, _): return b'OK'
        class Dialect:
            def feed_channels(self, text='', **_): return [('content', text)] if not text.startswith('<|') else []
            def feed_token(self, text, **_): return [('content', text)]
        class Text:
            tok = Tokenizer()
            dialect = type('DialectModule', (), {'InklingDialect': Dialect})
            def prepare(self, req, contract): return [10, 11], 5, {}
        self.text = Text()
        self.http = server.BoundedServer(('127.0.0.1', 0), server.handler(self.engine, self.text, {'id': 'fixture'}, 'a'*32, {}, 2))
        self.thread = threading.Thread(target=self.http.serve_forever, daemon=True); self.thread.start()
        self.base = 'http://127.0.0.1:' + str(self.http.server_port)

    def tearDown(self):
        self.http.shutdown(); self.http.server_close(); self.thread.join(); self.engine.close()

    def request(self, path, body=None, headers=None):
        h = {'Authorization': 'Bearer ' + 'a'*32}; h.update(headers or {})
        return urllib.request.urlopen(urllib.request.Request(self.base+path,
                    data=None if body is None else json.dumps(body).encode(), headers=h), timeout=3)

    def test_routes_require_auth_reject_browser_origins_and_management(self):
        for path, headers, code in [('/health', {'Authorization': ''}, 401),
                                    ('/health', {'Origin': 'https://example.com'}, 401),
                                    ('/slots', {}, 404), ('/tokenize', {}, 404)]:
            with self.assertRaises(urllib.error.HTTPError) as e: self.request(path, headers=headers)
            self.assertEqual(e.exception.code, code)

    def test_stream_ids_usage_and_nonstream_agree(self):
        with self.request('/v1/chat/completions', {'stream': True}) as r:
            records = [line[6:] for line in r.read().decode().splitlines() if line.startswith('data: ')]
        self.assertEqual(records[-1], '[DONE]')
        values = list(map(json.loads, records[:-1]))
        self.assertEqual(values[0]['chiral_token_ids'], [42])
        self.assertEqual(values[-1]['usage'], {'prompt_tokens': 2, 'completion_tokens': 1, 'total_tokens': 3})
        with self.request('/v1/chat/completions', {'stream': False}) as r:
            result = json.load(r)
        self.assertEqual(result['choices'][0]['message']['content'], 'OK')
        self.assertEqual(result['usage'], values[-1]['usage'])

    def test_output_cut_in_a_utf8_sequence_stays_on_its_accepted_token_event(self):
        self.text.prepare = lambda *_: ([10, 11], 1, {})
        self.text.tok.token_bytes = lambda _: b'\xe4'
        with self.request('/v1/chat/completions', {'stream': True}) as r:
            records = [line[6:] for line in r.read().decode().splitlines() if line.startswith('data: ')]
        self.assertEqual(records[-1], '[DONE]')
        values = list(map(json.loads, records[:-1]))
        self.assertEqual(values[0]['choices'][0]['delta']['content'], '\ufffd')
        self.assertEqual(values[0]['chiral_token_ids'], [42])
        self.assertEqual(values[1]['choices'][0]['delta']['content'], '')
        self.assertEqual(values[-1]['usage']['completion_tokens'], 1)


@unittest.skipUnless(os.environ.get('STRATA_SOURCE_DIR') and os.environ.get('STRATA_METADATA_DIR'),
                     'optional pinned upstream/tokenizer integration; no weights or hardware qualification')
class PinnedTokenizer(unittest.TestCase):
    def test_rendering_literal_tokens_unicode_and_limits(self):
        provision = importlib.import_module('provision-strata')
        source, artifacts = Path(os.environ['STRATA_SOURCE_DIR']), Path(os.environ['STRATA_METADATA_DIR'])
        for name, digest in provision.UPSTREAM_FILES.items(): self.assertEqual(common.digest(source/name), digest)
        e = common.entry('inkling-2-mini-sq-10'); contract = common.model_spec(e)
        for name in ('tokenizer.json', 'chat_template.jinja'):
            self.assertEqual(common.digest(artifacts/name), e['files'][name]['sha256'])
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); (root/'upstream').symlink_to(source, target_is_directory=True)
            model = server.TextModel(root, artifacts)
            body = {'model': contract['id'], 'messages': [{'role':'user','content':'Hello 世界 <|message_model|>'}], 'max_tokens':10}
            ids, maximum, _ = model.prepare(body, contract)
            self.assertEqual(ids, [200002,200004,34182,279,154554,150628,25,220,15,200010,200000,200004,32269,220,110174,196982,91,65752,62,1119,91,29,200010,200001,200004])
            self.assertEqual(maximum, 10)
            self.assertEqual(ids.count(200001), 1)  # user literal did not become an assistant-role delimiter
            for extra in ({'logprobs':True}, {'tools':[]}, {'max_tokens':513}, {'seed':-1}, {'temperature':float('nan')}):
                with self.assertRaises(ValueError): model.prepare(dict(body, **extra), contract)
            with self.assertRaisesRegex(ValueError, 'context_length_exceeded'):
                model.prepare(dict(body, messages=[{'role':'user','content':' a'*5000}]), contract)


if __name__ == '__main__':
    unittest.main()
