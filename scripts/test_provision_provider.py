"""Provisioning tests. The bearer tests create no deployment; the ingress tests generate one
in a temporary directory with the artifact and runtime validators stubbed. None is
qualification or deployment evidence."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import shutil
import sys
import tempfile
import unittest
from unittest import mock

# provision-provider imports its sibling scripts by module name.
sys.path.insert(0, str(Path(__file__).parent))
SPEC = importlib.util.spec_from_file_location('provision_provider_test', Path(__file__).with_name('provision-provider.py'))
provision = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(provision)

HOST = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}.get(platform.system())


@unittest.skipIf(os.name == 'nt' or HOST is None, 'owner-only secret fixtures are created with Unix modes')
class RpcBearerProvisioning(unittest.TestCase):
    """specify-chain-rpc-gateway task 2.5: the gateway refuses an inbound bearer equal to its
    upstream bearer, so provisioning must refuse it before creating the deployment."""

    def provision(self, inbound, upstream):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        def secret(name, value):
            path = root / name
            path.write_text(value + '\n')
            path.chmod(0o600)
            return str(path)
        runtime_dir = root / 'runtime'
        runtime_dir.mkdir()
        (runtime_dir / 'chiral-runtime.json').write_text('{}')
        inputs = {
            'release_id': 'r1', 'policy_public_key': '11' * 32,
            # Absent on purpose: a run that passes the bearer checks stops at the policy read.
            'version_policy_file': str(root / 'absent-policy.json'),
            'provider_domain': 'provider.example.org', 'relay_domain': 'relay.example.org',
            'rpc_domain': 'rpc.example.org', 'acme_email': 'ops@example.org',
            'network': {'identity': {'chain_id': 71, 'genesis_hash': provision.ESPACE_GENESIS, 'deployment_id': 'test'},
                        'rpc_url': 'https://evmtestnet.confluxrpc.com'},
            'platform_wallet': '0x' + '22' * 20, 'provider_key_file': secret('provider.key', '33' * 32),
            'model_dir': str(root / 'model'),
            'runtime_dir': str(runtime_dir), 'runtime': {'backend': 'cpu'},
            'input_wei_per_1k': '1', 'output_wei_per_1k': '1', 'min_funding_wei': '1',
            'max_concurrency': 1, 'max_output_tokens': 16, 'context_length': 128,
            'request_timeout_secs': 60, 'provider_p2p_port': 4001, 'relay_p2p_port': 4002, 'bootstrap': [],
        }
        if inbound is not None:
            inputs['rpc_token_file'] = secret('rpc.token', inbound)
        if upstream is not None:
            inputs['upstream_rpc_token_file'] = secret('upstream.token', upstream)
        (root / 'inputs.json').write_text(json.dumps(inputs))
        destination = root / 'deployment'
        argv = ['provision-provider.py', '--inputs', str(root / 'inputs.json'), '--output', str(destination), '--calibration']
        with mock.patch.object(sys, 'argv', argv), \
                mock.patch.object(provision, 'validate_runtime', return_value={'platform': HOST, 'backend': 'cpu'}):
            try:
                provision.main()
            except (ValueError, OSError) as error:
                return error, destination.exists()
        self.fail('provisioning unexpectedly succeeded')

    def test_identical_inbound_and_upstream_bearers_are_refused_before_the_destination(self):
        token = 'A' * 43
        error, created = self.provision(token, token)
        self.assertIsInstance(error, ValueError)
        self.assertIn('must differ from the upstream RPC bearer', str(error))
        self.assertNotIn(token, str(error))
        self.assertFalse(created)

    def test_distinct_or_absent_upstream_bearers_pass_the_bearer_checks(self):
        for upstream in ('B' * 43, None):
            with self.subTest(upstream=upstream):
                error, created = self.provision('A' * 43, upstream)
                # The run passed every bearer check and stopped at the absent policy file.
                self.assertIsInstance(error, FileNotFoundError)
                self.assertFalse(created)

    def test_missing_or_short_inbound_bearer_is_refused_in_calibration(self):
        for inbound, message in ((None, 'missing input fields: rpc_token_file'), ('A' * 31, 'at least 32 characters')):
            with self.subTest(inbound=inbound):
                error, created = self.provision(inbound, None)
                self.assertIsInstance(error, ValueError)
                self.assertIn(message, str(error))
                self.assertFalse(created)


def caddy_blocks(text):
    """The top-level blocks of a generated Caddyfile as (label, directive lines); the global
    options block has the label ''."""
    blocks, depth, label, body = [], 0, None, []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        if depth == 0:
            if not line.endswith('{'):
                raise AssertionError('directive outside a block: ' + line)
            label, body, depth = line[:-1].strip(), [], 1
            continue
        depth += line.count('{') - line.count('}')
        if depth == 0:
            blocks.append((label, body))
        else:
            body.append(line)
    if depth:
        raise AssertionError('unbalanced Caddyfile')
    return blocks


class ReferenceIngress(unittest.TestCase):
    """outbound-http-safeguards requirement 8 scenario "The reference ingress forwards only
    to the public listener" (task 3.8). Provisioning runs to completion for an embeddings
    deployment in a temporary directory, with the model and runtime validators stubbed and
    file protection left to the host defaults (neither is under test); the generated
    Caddyfile and provider configuration are then read."""
    PUBLIC_PATHS = ['/v1/*', '/healthz', '/readyz', '/api/version-policy']

    def deploy(self, calibration):
        old_umask = os.umask(0o022)
        os.umask(old_umask)
        self.addCleanup(os.umask, old_umask)
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        def secret(name, value):
            path = root / name
            path.write_text(value + '\n')
            path.chmod(0o600)
            return str(path)
        runtime_dir = root / 'runtime'
        runtime_dir.mkdir()
        (runtime_dir / 'chiral-runtime.json').write_text('{}')
        model_dir = root / 'model'
        model_dir.mkdir()
        (model_dir / 'chiral-artifact-sha256.json').write_text('{}')
        (root / 'policy.json').write_text(json.dumps({'signature': 'ab' * 64}))
        model = {'id': 'org/model/embed.gguf', 'revision': 'a' * 40, 'tokenizer_revision': 'a' * 40,
                 'chat_template_sha256': 'c' * 64, 'quantization': None, 'context_length': 128,
                 'max_output_tokens': 0, 'capabilities': ['embeddings'], 'serving': None}
        inputs = {
            'release_id': 'r1', 'policy_public_key': '11' * 32, 'version_policy_file': str(root / 'policy.json'),
            'provider_domain': 'provider.example.org', 'relay_domain': 'relay.example.org',
            'rpc_domain': 'rpc.example.org', 'acme_email': 'ops@example.org',
            'network': {'identity': {'chain_id': 71, 'genesis_hash': provision.ESPACE_GENESIS, 'deployment_id': 'test'},
                        'rpc_url': 'https://evmtestnet.confluxrpc.com'},
            'platform_wallet': '0x' + '22' * 20, 'provider_key_file': secret('provider.key', '33' * 32),
            'rpc_token_file': secret('rpc.token', 'A' * 43),
            'model_dir': str(model_dir), 'runtime_dir': str(runtime_dir),
            'runtime': {'backend': 'cpu', 'device_ids': [], 'memory_reserve_mib': 256, 'gpu_layers': 0,
                        'split_mode': 'none', 'tensor_split': [], 'cache_type_k': 'f16', 'cache_type_v': 'f16',
                        'flash_attention': False, 'batch_size': 512, 'ubatch_size': 512, 'startup_timeout_secs': 60},
            'input_wei_per_1k': '1', 'output_wei_per_1k': '0', 'min_funding_wei': '1',
            'max_concurrency': 1, 'max_output_tokens': 0, 'context_length': 128,
            'request_timeout_secs': 60, 'provider_p2p_port': 4001, 'relay_p2p_port': 4002, 'bootstrap': [],
        }
        (root / 'inputs.json').write_text(json.dumps(inputs))
        destination = root / 'deployment'
        argv = ['provision-provider.py', '--inputs', str(root / 'inputs.json'), '--output', str(destination)]
        if calibration:
            argv.append('--calibration')
        with mock.patch.object(sys, 'argv', argv), \
                mock.patch.object(provision, 'validate_runtime', return_value={'platform': HOST, 'backend': 'cpu'}), \
                mock.patch.object(provision, 'validate_artifact', return_value=(model, 'embeddings')), \
                mock.patch.object(provision, 'protect'), mock.patch.object(provision, 'private_file'), \
                contextlib.redirect_stdout(io.StringIO()):
            provision.main()
        return (destination / 'Caddyfile').read_text(), json.loads((destination / 'provider.json').read_text())

    def test_the_provider_site_forwards_only_public_router_paths_to_the_public_listener(self):
        caddyfile, provider = self.deploy(calibration=False)
        blocks = caddy_blocks(caddyfile)
        self.assertEqual([label for label, _ in blocks], ['', 'provider.example.org', 'relay.example.org', 'rpc.example.org'])
        # Implementation value (design decision 10): the Caddy administration API is off.
        self.assertIn('admin off', blocks[0][1])
        site = blocks[1][1]
        matchers = [line.split() for line in site if line.startswith('@')]
        self.assertEqual(matchers, [['@public', 'path'] + self.PUBLIC_PATHS])
        handles = [line for line in site if line.startswith('handle')]
        self.assertEqual(handles, ['handle @public {'])
        self.assertEqual([line for line in site if line.startswith('reverse_proxy')], ['reverse_proxy 127.0.0.1:9419 {'])
        # Anything that is not a public-router path is answered 404 by the site itself.
        self.assertEqual(site[-1], 'respond 404')
        for path in self.PUBLIC_PATHS:
            self.assertFalse(path.startswith('/admin') or path in ('/tokenize', '/apply-template', '/metrics'), path)
        # No site serves files or forwards to administration or the engine.
        upstreams = {}
        for label, body in blocks:
            for line in body:
                self.assertNotIn('file_server', line, label)
                self.assertFalse(line.startswith(('root ', 'php_fastcgi', 'import ')), label + ': ' + line)
                self.assertNotIn('9421', line, label)
                self.assertNotIn('8000', line, label)
                if line.startswith('reverse_proxy'):
                    upstreams.setdefault(label, []).append(line.split()[1])
        self.assertEqual(upstreams, {'provider.example.org': ['127.0.0.1:9419'],
                                     'relay.example.org': ['127.0.0.1:8081'],
                                     'rpc.example.org': ['127.0.0.1:8547']})
        # The reference topology (implementation values of design decision 10): the public
        # listener, administration and the engine all listen on loopback.
        self.assertEqual((provider['public_bind'], provider['admin_bind'], provider['models'][0]['engine_url']),
                         ('127.0.0.1:9419', '127.0.0.1:9421', 'http://127.0.0.1:8000'))
        self.assertEqual(provider['public_endpoint'], 'https://provider.example.org')

    def test_a_calibration_deployment_forwards_nothing(self):
        caddyfile, provider = self.deploy(calibration=True)
        self.assertNotIn('reverse_proxy', caddyfile)
        self.assertNotIn('file_server', caddyfile)
        self.assertEqual(provider['public_endpoint'], 'http://127.0.0.1:9419')


if __name__ == '__main__':
    unittest.main()
