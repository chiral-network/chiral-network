"""Provisioning input boundary tests; they create no deployment and are not qualification evidence."""
import importlib.util
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


if __name__ == '__main__':
    unittest.main()
