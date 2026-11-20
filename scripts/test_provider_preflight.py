"""Preflight boundary tests against a loopback fixture; not deployment or qualification evidence."""
import http.server
import importlib.util
import json
from pathlib import Path
import sys
import threading
import unittest

# provider-preflight imports its sibling scripts by module name.
sys.path.insert(0, str(Path(__file__).parent))
SPEC = importlib.util.spec_from_file_location('provider_preflight', Path(__file__).with_name('provider-preflight.py'))
preflight = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(preflight)


class Gateway(http.server.BaseHTTPRequestHandler):
    """Answers every request with the class's status and records what it received."""
    status = 401
    seen = []

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        type(self).seen.append((self.path, self.headers.get('Authorization'), self.headers.get('Origin'), json.loads(body)))
        self.send_response(self.status)
        if self.status in (301, 302, 307, 308):
            self.send_header('Location', 'http://127.0.0.1:9/api/chain/rpc')
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', '2')
        self.end_headers()
        self.wfile.write(b'{}')

    def log_message(self, *args):
        pass


class RpcGatewayBearerProbe(unittest.TestCase):
    def probe(self, status):
        Gateway.status = status
        Gateway.seen = []
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Gateway)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            preflight.check_rpc_gateway_bearer('http://127.0.0.1:%d' % server.server_address[1])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_refusal_without_bearer_passes(self):
        self.probe(401)
        self.assertEqual(len(Gateway.seen), 1)
        path, authorization, origin, body = Gateway.seen[0]
        self.assertEqual(path, '/api/chain/rpc')
        self.assertIsNone(authorization)
        self.assertIsNone(origin)
        self.assertEqual(body, {'jsonrpc': '2.0', 'id': 1, 'method': 'eth_chainId', 'params': []})

    def test_gateway_answering_without_bearer_fails(self):
        for status in (200, 400, 403, 404, 429, 500, 502):
            with self.subTest(status=status), self.assertRaisesRegex(RuntimeError, 'HTTP %d, not 401' % status):
                self.probe(status)

    def test_redirect_is_not_followed(self):
        with self.assertRaises(ValueError):
            self.probe(302)
        self.assertEqual(len(Gateway.seen), 1)


class RuntimeObservationLiveness(unittest.TestCase):
    """`GET /admin/runtime` reports engine process liveness as `alive` and readiness apart
    (provider-daemon-lifecycle requirement 2). Running preflight takes the observation of a live
    managed engine and reads readiness from `/readyz`."""
    observation = {'pid': 123, 'model_id': 'org/model/Q4.gguf'}

    def row(self, **fields):
        return dict({'model_id': 'org/model/Q4.gguf', 'managed': True, 'alive': True, 'ready': True,
                     'observation': self.observation}, **fields)

    def test_a_live_engine_is_accepted_by_its_liveness(self):
        for ready in (True, False):
            with self.subTest(ready=ready):
                self.assertEqual(preflight.live_runtime_observation([self.row(ready=ready)]), self.observation)

    def test_a_dead_unmanaged_or_unobserved_engine_is_refused(self):
        # An older daemon reported liveness as `ready` and made no `alive` claim.
        older = self.row()
        del older['alive']
        for rows in ([self.row(alive=False, ready=False)], [self.row(alive=False, ready=True)], [older],
                     [self.row(managed=False, alive=None, observation=None)], [self.row(observation=None)],
                     [], [self.row(), self.row()]):
            with self.subTest(rows=rows), self.assertRaisesRegex(RuntimeError, 'not alive/observed'):
                preflight.live_runtime_observation(rows)


if __name__ == '__main__':
    unittest.main()
