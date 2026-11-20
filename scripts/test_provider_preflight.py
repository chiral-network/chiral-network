"""Preflight boundary tests against a loopback fixture; not deployment or qualification evidence."""
import http.server
import importlib.util
import json
from pathlib import Path
import re
import sys
import threading
import unittest
from unittest import mock

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


class Ingress(http.server.BaseHTTPRequestHandler):
    """Answers each GET with the status that `statuses` gives its path, 404 otherwise, and
    records the path."""
    statuses = {}
    seen = []

    def do_GET(self):
        type(self).seen.append(self.path)
        status = self.statuses.get(self.path, 404)
        self.send_response(status)
        if status in (301, 302, 307, 308):
            self.send_header('Location', 'http://127.0.0.1:9/admin/status')
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', '2')
        self.end_headers()
        self.wfile.write(b'{}')

    def log_message(self, *args):
        pass


class PrivateRouteProbe(unittest.TestCase):
    """outbound-http-safeguards requirement 8 scenario "Preflight refuses an exposed
    administration or engine route" (task 3.8): the public-route probe of
    `provider-preflight.py --running`, against a loopback fixture standing in for the
    provider's public endpoint."""
    ROUTES = ['/admin/status', '/admin/runtime', '/tokenize', '/apply-template', '/metrics', '/api/headless/wallet']

    def probe(self, statuses):
        Ingress.statuses = statuses
        Ingress.seen = []
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Ingress)
        thread = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': 0.05}, daemon=True)
        thread.start()
        try:
            preflight.check_private_routes('http://127.0.0.1:%d' % server.server_address[1])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_an_exposed_administration_or_engine_route_fails_and_is_named(self):
        # The scenario's routes first, then the other private or retired routes probed.
        for route in self.ROUTES:
            with self.subTest(route=route), self.assertRaisesRegex(
                    RuntimeError, '^private or retired route exposed: %s$' % re.escape(route)):
                self.probe({route: 200})

    def test_any_answer_but_a_refusal_fails(self):
        # A route that ingress forwards is reachable whatever its upstream answers: an empty
        # success, an upstream error, or Caddy's 502 for a forwarded route whose upstream is
        # down. Only the refusals of design decision 10 pass (an implementation value).
        for route in ('/admin/status', '/tokenize'):
            for status in (204, 400, 500, 502):
                with self.subTest(route=route, status=status), self.assertRaisesRegex(
                        RuntimeError, '^private or retired route exposed: %s$' % re.escape(route)):
                    self.probe({route: status})

    def test_every_route_is_probed_and_a_refusal_passes(self):
        # Implementation value (design decision 10): 401, 403, 404 and 405 count as refused.
        for status in (401, 403, 404, 405):
            with self.subTest(status=status):
                self.probe({route: status for route in self.ROUTES})
                self.assertEqual(Ingress.seen, self.ROUTES)

    def test_a_redirecting_route_is_not_followed_and_fails(self):
        with self.assertRaisesRegex(ValueError, 'must not redirect'):
            self.probe({'/admin/runtime': 302})
        self.assertEqual(Ingress.seen, ['/admin/status', '/admin/runtime'])


class ListenerCheck(unittest.TestCase):
    """outbound-http-safeguards requirement 8 scenario "Preflight refuses a private listener
    on a non-loopback address" (task 3.8): `check_listeners` with the listener table stubbed
    in each platform's format. The private ports are the engine 8000, the public listener
    9419, administration 9421, the chain RPC gateway 8547 and the relay 8081."""
    PRIVATE = (8000, 9419, 9421, 8547, 8081)
    COMMANDS = {
        'Linux': ['ss', '-H', '-ltn'],
        'Darwin': ['lsof', '-nP', '-iTCP', '-sTCP:LISTEN', '-Fn'],
    }

    @staticmethod
    def table(system, listeners):
        """`listeners`, (address, port) pairs, as the platform's listing prints them."""
        def endpoint(address, port):
            return ('[%s]' % address if ':' in address else address) + ':%d' % port
        if system == 'Linux':
            return ''.join('LISTEN 0      4096   %s   0.0.0.0:*\n' % endpoint(*listener) for listener in listeners)
        if system == 'Darwin':
            return ''.join('p4242\nf7\nn%s\n' % endpoint(*listener) for listener in listeners)
        return json.dumps([{'LocalAddress': address, 'LocalPort': port} for address, port in listeners])

    def check(self, system, listeners):
        with mock.patch.object(preflight.platform, 'system', return_value=system), \
                mock.patch.object(preflight, 'run', return_value=self.table(system, listeners)) as run:
            preflight.check_listeners()
        command = run.call_args.args[0]
        if system in self.COMMANDS:
            self.assertEqual(command, self.COMMANDS[system])
        else:
            self.assertEqual(command[0], 'powershell.exe')

    def test_a_private_port_bound_to_a_non_loopback_address_fails(self):
        for system in ('Linux', 'Darwin', 'Windows'):
            wildcards = ('0.0.0.0', '::', '192.168.1.5') + (('*',) if system != 'Windows' else ())
            for address in wildcards:
                for port in self.PRIVATE:
                    with self.subTest(system=system, address=address, port=port), \
                            self.assertRaisesRegex(RuntimeError, '^private service bound to non-loopback address$'):
                        self.check(system, [('127.0.0.1', 9421), (address, port), ('127.0.0.1', 8000)])

    def test_loopback_private_listeners_and_public_ingress_pass(self):
        for system in ('Linux', 'Darwin', 'Windows'):
            with self.subTest(system=system):
                self.check(system, [('127.0.0.1', port) for port in self.PRIVATE]
                           + [('::1', port) for port in self.PRIVATE]
                           + [('0.0.0.0', 443), ('::', 443), ('0.0.0.0', 4001)])


if __name__ == '__main__':
    unittest.main()
