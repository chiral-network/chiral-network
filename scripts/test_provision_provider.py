"""Provisioning tests. The bearer tests create no deployment; the ingress tests generate one
in a temporary directory with the artifact and runtime validators stubbed. None is
qualification or deployment evidence."""
import ast
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock
import zipfile

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



# application-startup-boundaries task 3.5 (requirement 5): native provisioning starts no service.
# The real GGUF and runtime fixture builders of the artifact tests are reused.
import test_provision_artifacts as artifacts  # noqa: E402

PROCESS_EVENTS = {'subprocess.Popen', 'os.system', 'os.posix_spawn', 'os.exec', 'os.fork', 'os.forkpty',
                  'os.spawn', 'os.startfile', 'pty.spawn'}
NETWORK_EVENTS = {'socket.connect', 'socket.bind', 'socket.sendto', 'urllib.Request', 'http.client.connect'}
# Filesystem changes, by audit event: the argument positions of the changed paths, and of the
# directory descriptor that a relative path is resolved against, if any.
CHANGE_EVENTS = {'os.mkdir': ((0,), 2), 'os.rename': ((0, 1), 2), 'os.symlink': ((1,), 2), 'os.link': ((0, 1), 2),
                 'os.remove': ((0,), 1), 'os.rmdir': ((0,), 1), 'os.truncate': ((0,), None), 'os.chmod': ((0,), 2),
                 'os.chown': ((0,), 3), 'os.utime': ((0,), 3), 'os.chflags': ((0,), None), 'os.lchflags': ((0,), None),
                 'os.setxattr': ((0,), None), 'os.removexattr': ((0,), None), 'os.mkfifo': ((0,), 2), 'os.mknod': ((0,), 3),
                 'shutil.copyfile': ((1,), None), 'shutil.copymode': ((1,), None), 'shutil.copystat': ((1,), None),
                 'shutil.copytree': ((1,), None), 'shutil.rmtree': ((0,), 1), 'shutil.move': ((0, 1), None)}
WRITE_FLAGS = os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND


class ProvisioningAudit:
    """Records, while active, every process creation, network connection or bind and filesystem
    change that Python code makes, through an audit hook (sys.addaudithook), and every call of
    subprocess.run and subprocess.Popen, which it replaces: `run` returns `answer(argv)` (or
    refuses when no answer is given) and `Popen` always refuses, as a process that could outlive
    the script. The order of calls, their returns and the script's own return is kept."""
    hook_installed = False
    active = None

    def __init__(self, answer=None):
        self.answer = answer
        self.processes, self.network, self.changes, self.order = [], [], [], []
        self.rmtree_roots = []

    @classmethod
    def hook(cls, event, args):
        audit = cls.active
        if audit is not None:
            audit.event(event, args)

    def event(self, name, args):
        if name in PROCESS_EVENTS:
            self.processes.append((name, repr(args)[:200]))
        elif name in NETWORK_EVENTS:
            self.network.append((name, repr(args)[:200]))
        elif name == 'open':
            path, mode, flags = args
            writes = (isinstance(mode, str) and any(c in mode for c in 'wax+')) or (mode is None and flags & WRITE_FLAGS)
            if writes and not isinstance(path, int):
                self.changes.append((name, os.path.abspath(os.fsdecode(path))))
        elif name in CHANGE_EVENTS:
            positions, dir_fd = CHANGE_EVENTS[name]
            relative_to = args[dir_fd] if dir_fd is not None and dir_fd < len(args) else None
            for position in positions:
                path = args[position]
                if isinstance(path, int):
                    continue
                path = os.fsdecode(path)
                if relative_to is not None and not os.path.isabs(path):
                    # Inside a descriptor-based tree removal: the tree is the last one removed.
                    self.changes.append((name, self.rmtree_roots[-1] if self.rmtree_roots else '<unresolved ' + path + '>'))
                else:
                    self.changes.append((name, os.path.abspath(path)))
            if name == 'shutil.rmtree':
                self.rmtree_roots.append(os.path.abspath(os.fsdecode(args[0])))

    def run(self, args, *rest, **options):
        argv = [str(arg) for arg in args]
        self.order.append(('start', argv))
        if self.answer is None:
            self.order.append(('refused', argv))
            raise AssertionError('subprocess.run called: ' + repr(argv))
        result = self.answer(argv, options)
        self.order.append(('end', argv))
        return result

    def popen(self, args, *rest, **options):
        self.order.append(('Popen', [str(arg) for arg in args]))
        raise AssertionError('subprocess.Popen called: ' + repr(args))

    @contextlib.contextmanager
    def active_during(self):
        if not ProvisioningAudit.hook_installed:
            sys.addaudithook(ProvisioningAudit.hook)
            ProvisioningAudit.hook_installed = True
        dont_write_bytecode = sys.dont_write_bytecode
        sys.dont_write_bytecode = True
        with mock.patch.object(subprocess, 'run', self.run), mock.patch.object(subprocess, 'Popen', self.popen):
            ProvisioningAudit.active = self
            try:
                yield self
            finally:
                ProvisioningAudit.active = None
                sys.dont_write_bytecode = dont_write_bytecode
        self.order.append(('script returned', None))

    def outside(self, *roots):
        roots = [os.path.realpath(root) for root in roots]
        def inside(path):
            path = os.path.realpath(path)
            return any(path == root or path.startswith(root + os.sep) for root in roots)
        return [(name, path) for name, path in self.changes if not inside(path)]


def snapshot(root, skip=()):
    """Every path under `root` but those under `skip`, with the SHA-256 of each file."""
    found = {}
    for directory, directories, files in os.walk(root):
        directories[:] = [d for d in directories if os.path.join(directory, d) not in skip]
        for name in directories:
            found[os.path.join(directory, name)] = 'directory'
        for name in files:
            path = os.path.join(directory, name)
            found[path] = hashlib.sha256(Path(path).read_bytes()).hexdigest()
    return found


class ProvisioningAuditSelfTest(unittest.TestCase):
    """The audit sees what it must catch: a started process, a connection, a file written
    outside the allowed root, and calls of subprocess.run and Popen."""

    def test_the_audit_records_processes_connections_and_writes(self):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        audit = ProvisioningAudit()
        with audit.active_during():
            with self.assertRaises(AssertionError):
                subprocess.run(['launch-something'])
            with self.assertRaises(AssertionError):
                subprocess.Popen(['launch-something'])
            pid = os.posix_spawn('/usr/bin/true', ['true'], {}) if hasattr(os, 'posix_spawn') else None
            (root / 'outside.txt').write_text('x')
            import socket
            with socket.socket() as probe:
                try:
                    probe.connect(('127.0.0.1', 9))
                except OSError:
                    pass
        if pid is not None:
            os.waitpid(pid, 0)
            self.assertIn('os.posix_spawn', [name for name, _ in audit.processes])
        self.assertEqual([kind for kind, _ in audit.order], ['start', 'refused', 'Popen', 'script returned'])
        self.assertIn(('open', str(root / 'outside.txt')), audit.outside(root / 'allowed'))
        self.assertIn('socket.connect', [name for name, _ in audit.network])


def host_runtime():
    """The host's platform and architecture, and a native header of that architecture."""
    architecture = {'arm64': 'aarch64', 'aarch64': 'aarch64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}[platform.machine()]
    if HOST == 'macos':
        return HOST, architecture, artifacts.macho(0x0100000c if architecture == 'aarch64' else 0x01000007)
    return HOST, architecture, artifacts.elf(183 if architecture == 'aarch64' else 62)


def runtime_archive(path):
    """A zip runtime package holding only llama-server for the host, and its SHA-256."""
    _, _, header = host_runtime()
    with zipfile.ZipFile(path, 'w') as archive:
        archive.writestr('llama-server', header)
    return hashlib.sha256(path.read_bytes()).hexdigest()


@unittest.skipIf(os.name == 'nt' or HOST is None, 'owner-only secret fixtures are created with Unix modes')
class NativeProvisioningStartsNoService(unittest.TestCase):
    """application-startup-boundaries requirement 5, scenarios "Provider provisioning leaves
    nothing running" and "Runtime and model provisioning start nothing". Each script runs to
    completion in this process on real fixtures (a runtime package for this host installed by
    provision-runtime.py, an embedding GGUF artifact verified by provision-model.py's
    validator), without --container (design decision 8), under ProvisioningAudit."""

    def setUp(self):
        old_umask = os.umask(0o022)
        os.umask(old_umask)
        self.addCleanup(os.umask, old_umask)
        self.root = Path(tempfile.mkdtemp()).resolve()
        self.addCleanup(shutil.rmtree, self.root)

    def provider_inputs(self):
        root = self.root
        def secret(name, value):
            path = root / name
            path.write_text(value + '\n')
            path.chmod(0o600)
            return str(path)
        plat, architecture, _ = host_runtime()
        archive = root / 'runtime.zip'
        artifacts.runtime.install_archive(archive, root / 'runtime', runtime_archive(archive), plat, architecture, 'cpu')
        embedding = artifacts.gguf_bytes(extra={'llama.pooling_type': (4, 1), 'llama.attention.causal': (7, 0)})
        artifacts.make_artifact(root / 'model', shards={'embed.gguf': embedding}, task='embeddings')
        (root / 'policy.json').write_text(json.dumps({'signature': 'ab' * 64}))
        inputs = {
            'release_id': 'r1', 'policy_public_key': '11' * 32, 'version_policy_file': str(root / 'policy.json'),
            'provider_domain': 'provider.example.org', 'relay_domain': 'relay.example.org',
            'rpc_domain': 'rpc.example.org', 'acme_email': 'ops@example.org',
            'network': {'identity': {'chain_id': 71, 'genesis_hash': provision.ESPACE_GENESIS, 'deployment_id': 'test'},
                        'rpc_url': 'https://evmtestnet.confluxrpc.com'},
            'platform_wallet': '0x' + '22' * 20, 'provider_key_file': secret('provider.key', '33' * 32),
            'rpc_token_file': secret('rpc.token', 'A' * 43), 'upstream_rpc_token_file': secret('upstream.token', 'B' * 43),
            'model_dir': str(root / 'model'), 'runtime_dir': str(root / 'runtime'),
            'runtime': {'backend': 'cpu', 'device_ids': [], 'memory_reserve_mib': 256, 'gpu_layers': 0,
                        'split_mode': 'none', 'tensor_split': [], 'cache_type_k': 'f16', 'cache_type_v': 'f16',
                        'flash_attention': False, 'batch_size': 512, 'ubatch_size': 512, 'startup_timeout_secs': 60},
            'input_wei_per_1k': '1', 'output_wei_per_1k': '0', 'min_funding_wei': '1',
            'max_concurrency': 1, 'max_output_tokens': 0, 'context_length': 512,
            'request_timeout_secs': 60, 'provider_p2p_port': 4001, 'relay_p2p_port': 4002, 'bootstrap': [],
        }
        (root / 'inputs.json').write_text(json.dumps(inputs))
        return inputs

    def provision_provider(self, audit, windows=False):
        """Runs provision-provider.py's main() natively, with `os.name` seen as 'nt' by the
        script itself when `windows` (its access-control branch); returns the destination and
        what it printed."""
        destination = self.root / 'deployment'
        argv = ['provision-provider.py', '--inputs', str(self.root / 'inputs.json'), '--output', str(destination)]
        script_os = types.SimpleNamespace(**{name: getattr(os, name) for name in dir(os) if not name.startswith('__')})
        script_os.name = 'nt' if windows else os.name
        output = io.StringIO()
        with mock.patch.object(sys, 'argv', argv), mock.patch.object(provision, 'os', script_os), \
                contextlib.redirect_stdout(output), audit.active_during():
            provision.main()
        return destination, output.getvalue()

    def test_native_provider_provisioning_runs_no_process_and_writes_only_its_destination(self):
        """On this host's own branch, provisioning runs no process at all (no subprocess call,
        no process audit event), opens no connection and binds no socket, changes nothing
        outside the destination (the audit, and the rest of the test root byte for byte), says
        that no services were started, and its destination holds configuration, secrets and
        empty data directories only, nothing that registers a service or a start at login.
        Would catch a service, engine, node or miner started or scheduled, and a file written
        elsewhere."""
        self.provider_inputs()
        before = snapshot(self.root)
        audit = ProvisioningAudit()
        destination, printed = self.provision_provider(audit)
        self.assertEqual(audit.processes, [])
        self.assertEqual(audit.order, [('script returned', None)])
        self.assertEqual(audit.network, [])
        self.assertEqual(audit.outside(destination), [])
        self.assertGreater(len(audit.changes), 10, 'the audit saw no write')
        self.assertEqual(snapshot(self.root, skip={str(destination)}), before)
        self.assertIn('no services started', printed)
        created = sorted(str(path.relative_to(destination)) for path in destination.rglob('*') if path.is_file())
        self.assertEqual(created, ['Caddyfile', 'provider.json', 'relay.json', 'rpc-gateway.json',
                                   'secrets/admin.token', 'secrets/engine.token', 'secrets/provider.key',
                                   'secrets/rpc.token', 'secrets/upstream-rpc.token', 'version-policy.json'])
        self.assertEqual(sorted(str(path.relative_to(destination)) for path in (destination / 'data').iterdir()),
                         ['data/caddy', 'data/provider', 'data/relay', 'data/rpc', 'data/tls'])
        self.assertFalse(any((destination / 'data').rglob('*.*')))

    def test_windows_access_control_helpers_are_the_only_processes_and_each_returns_first(self):
        """With the script's own `os.name` seen as 'nt', the only processes it asks for are the
        access-control helpers (`whoami`, `icacls` on paths inside the destination, and the
        PowerShell ACL check of each secret input), each through subprocess.run, which returns
        before the next starts and before the script returns; Popen is never called, nothing is
        written outside the destination and no service is started. Would catch any other
        program, a helper left running (Popen) or one run on a path outside the destination."""
        inputs = self.provider_inputs()
        identity = 'HOST\\operator'
        def answer(argv, options):
            self.assertTrue(options.get('check') and options.get('capture_output'), argv)
            return subprocess.CompletedProcess(argv, 0, identity + '\n' if argv == ['whoami'] else '', '')
        audit = ProvisioningAudit(answer)
        destination, printed = self.provision_provider(audit, windows=True)
        self.assertIn('no services started', printed)
        self.assertEqual(audit.processes, [])
        self.assertEqual(audit.outside(destination), [])
        starts = [argv for kind, argv in audit.order if kind == 'start']
        self.assertGreater(len(starts), 10)
        # Each helper returns before the next starts, and all before the script returns.
        expected = []
        for argv in starts:
            expected += [('start', argv), ('end', argv)]
        self.assertEqual(audit.order, expected + [('script returned', None)])
        secrets_checked = []
        for argv in starts:
            program = argv[0]
            if program == 'whoami':
                self.assertEqual(argv, ['whoami'])
            elif program == 'icacls':
                self.assertTrue(argv[1] == str(destination) or argv[1].startswith(str(destination) + os.sep), argv)
                self.assertIn(argv[2:], (['/setowner', identity], ['/inheritance:r', '/grant:r', identity + ':F']))
            elif program == 'powershell.exe':
                self.assertEqual(argv[1:4], ['-NoProfile', '-NonInteractive', '-Command'])
                # The script reads one access-control list: its only cmdlet is Get-Acl.
                self.assertEqual(set(re.findall(r'\b[A-Z][a-z]+-[A-Z][A-Za-z]+\b', argv[4])), {'Get-Acl'})
                secrets_checked.append(argv[5])
            else:
                self.fail('unexpected program ' + repr(argv))
        self.assertEqual(sorted(secrets_checked), sorted(str(Path(inputs[field]).absolute()) for field in
                                                         ('provider_key_file', 'rpc_token_file', 'upstream_rpc_token_file')))

    def test_runtime_provisioning_from_an_archive_or_a_download_starts_nothing(self):
        """provision-runtime.py's main(), from a local --archive and from an official --url
        (the download stubbed, the compiled pin patched for this test's package), runs no
        process, opens no connection, binds no socket and writes only its output and, for the
        download, its own temporary directory, which it removes. Would catch an installer that
        runs the server or registers it as a service."""
        plat, architecture, _ = host_runtime()
        archive = self.root / 'runtime.zip'
        digest = runtime_archive(archive)
        temporary = self.root / 'tmp'
        temporary.mkdir()
        name = 'chiral-test-runtime.zip'
        url = 'https://github.com/ggml-org/llama.cpp/releases/download/b12110/' + name
        package = archive.read_bytes()

        class Download(io.BytesIO):
            def __init__(self):
                super().__init__(package)
                self.url = url
        for source in ('archive', 'url'):
            with self.subTest(source=source):
                output = self.root / ('runtime-from-' + source)
                argv = ['provision-runtime.py', '--sha256', digest, '--source-revision', artifacts.runtime.SOURCE_REVISION,
                        '--output', str(output), '--platform', plat, '--architecture', architecture, '--backend', 'cpu']
                argv += ['--archive', str(archive)] if source == 'archive' else ['--url', url]
                before = snapshot(self.root)
                audit = ProvisioningAudit()
                printed = io.StringIO()
                with mock.patch.object(sys, 'argv', argv), mock.patch.object(tempfile, 'tempdir', str(temporary)), \
                        mock.patch.dict(artifacts.runtime.OFFICIAL_ASSETS, {name: (digest, plat, architecture, 'cpu')}), \
                        mock.patch.object(artifacts.runtime.urllib.request, 'urlopen', lambda *args, **kwargs: Download()), \
                        contextlib.redirect_stdout(printed), audit.active_during():
                    artifacts.runtime.main()
                self.assertEqual((audit.processes, audit.network), ([], []))
                self.assertEqual(audit.order, [('script returned', None)])
                self.assertEqual(audit.outside(output, temporary), [])
                self.assertTrue(any(path.startswith(str(output)) for _, path in audit.changes))
                if source == 'url':
                    self.assertTrue(any(path.startswith(str(temporary)) for _, path in audit.changes), 'the download was not written')
                self.assertEqual(list(temporary.iterdir()), [])
                self.assertEqual(snapshot(self.root, skip={str(output)}), before)
                self.assertRegex(printed.getvalue(), r'runtime_sha256=[0-9a-f]{64}')
                self.assertEqual(sorted(path.name for path in output.iterdir()), ['chiral-runtime.json', 'llama-server'])

    def test_model_provisioning_starts_nothing(self):
        """provision-model.py's main(), with a stand-in for the Hub client that serves one
        pinned GGUF from a local cache, runs no process, opens no connection, binds no socket and
        writes only its output, which holds the verified artifact only. Would catch a model
        installer that starts an engine or a service."""
        data = artifacts.gguf_bytes(template='{{ messages }}')
        cache = self.root / 'hub-cache'
        cache.mkdir()
        (cache / 'model.gguf').write_bytes(data)
        revision = 'c' * 40
        info = types.SimpleNamespace(sha=revision, card_data={'license': 'mit'}, siblings=[types.SimpleNamespace(
            rfilename='model.gguf', blob_id=None, lfs=types.SimpleNamespace(sha256=hashlib.sha256(data).hexdigest()))])
        hub = types.ModuleType('huggingface_hub')
        hub.HfApi = lambda: types.SimpleNamespace(model_info=lambda repo_id, revision, files_metadata: info)
        hub.hf_hub_download = lambda repo_id, filename, revision: str(cache / filename)
        output = self.root / 'model'
        argv = ['provision-model.py', '--output', str(output), '--repo', 'owner/model-GGUF', '--revision', revision,
                '--task', 'generation', '--context-length', '512', '--gguf-file', 'model.gguf', '--accept-license', 'mit']
        before = snapshot(self.root)
        audit = ProvisioningAudit()
        printed = io.StringIO()
        with mock.patch.dict(sys.modules, {'huggingface_hub': hub}), mock.patch.object(sys, 'argv', argv), \
                contextlib.redirect_stdout(printed), audit.active_during():
            artifacts.model.main()
        self.assertEqual((audit.processes, audit.network), ([], []))
        self.assertEqual(audit.order, [('script returned', None)])
        self.assertEqual(audit.outside(output), [])
        self.assertGreater(len(audit.changes), 3)
        self.assertEqual(snapshot(self.root, skip={str(output)}), before)
        self.assertIn('artifact_manifest_sha256=', printed.getvalue())
        self.assertEqual(sorted(path.name for path in output.iterdir()),
                         ['chiral-artifact-sha256.json', 'chiral-chat-template.jinja', 'chiral-model.json', 'model.gguf'])


# Modules whose use creates a process, runs a server or opens a program, and the process
# functions of `os` and `asyncio`, for the static scan of the provisioning scripts. Scheduler and
# start-at-login names are refused in every script by the whole-file scan of
# application_startup_boundaries_test.rs (`nothing_writes_a_login_entry_or_runs_a_service_manager`).
PROCESS_MODULES = {'subprocess', 'multiprocessing', 'pty', 'pexpect', 'sh', 'plumbum', 'socketserver', 'http.server',
                   'xmlrpc.server', 'wsgiref.simple_server', 'webbrowser', 'ctypes', 'cffi', 'concurrent.futures.process',
                   'asyncio.subprocess', 'daemon', 'service', 'launchd', 'win32service', 'win32serviceutil', 'winreg',
                   '_winreg', 'systemd', 'dbus'}
OS_PROCESS_FUNCTIONS = {'system', 'popen', 'fork', 'forkpty', 'posix_spawn', 'posix_spawnp', 'execl', 'execle', 'execlp',
                        'execlpe', 'execv', 'execve', 'execvp', 'execvpe', 'spawnl', 'spawnle', 'spawnlp', 'spawnlpe',
                        'spawnv', 'spawnve', 'spawnvp', 'spawnvpe', 'startfile', 'setsid'}
ASYNC_PROCESS_FUNCTIONS = {'create_subprocess_exec', 'create_subprocess_shell', 'subprocess_exec', 'subprocess_shell',
                           'start_server', 'start_unix_server', 'create_server'}
PROVISIONING_SCRIPTS = ('provision-provider.py', 'provision-runtime.py', 'provision-model.py')


def process_constructs(source):
    """Every construct of the Python `source` that can create a process, start a server or load
    code by a computed name, as (line, description), in source order: an import of a process,
    server or service module (under any alias), an `os` or `asyncio` process function (as an
    attribute or an imported name), any use of the `subprocess` module (a `subprocess.run` call
    described with its program, its function, whether only `os.name == 'nt'` reaches it, and
    its true keywords), `__import__`, `eval`, `exec`, `compile`, `getattr` or `vars` of a module,
    and every module loaded by name or path (described by that name)."""
    tree = ast.parse(source)
    parents = {}
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            parents[child] = node

    def ancestors(node):
        while node in parents:
            child, node = node, parents[node]
            yield child, node

    def function_of(node):
        return next((parent.name for _, parent in ancestors(node) if isinstance(parent, (ast.FunctionDef, ast.AsyncFunctionDef))), '<module>')

    def windows_only(node):
        for child, parent in ancestors(node):
            if isinstance(parent, ast.If) and child in parent.body and ast.unparse(parent.test) == "os.name == 'nt'":
                return True
        return False

    def forbidden_module(name):
        return any(name == module or name.startswith(module + '.') for module in PROCESS_MODULES)

    def constants(node):
        return [child.value for child in ast.walk(node) if isinstance(child, ast.Constant) and isinstance(child.value, str)]

    # `import os as o`: the attribute scan below reads `o.fork` as `os.fork`.
    aliases = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name in ('os', 'asyncio', 'subprocess', 'importlib', 'builtins', 'sys', 'runpy'):
                    aliases[alias.asname or alias.name] = alias.name

    def module_of(node):
        text = ast.unparse(node)
        head, _, rest = text.partition('.')
        return aliases.get(head, head) + ('.' + rest if rest else '')

    found = []
    for node in ast.walk(tree):
        line = getattr(node, 'lineno', 0)
        if isinstance(node, ast.Import):
            for alias in node.names:
                if forbidden_module(alias.name):
                    found.append((line, 'import ' + alias.name + (' as ' + alias.asname if alias.asname else '')))
        elif isinstance(node, ast.ImportFrom):
            module = node.module or ''
            names = [alias.name for alias in node.names]
            if forbidden_module(module) or (module == 'os' and ('*' in names or OS_PROCESS_FUNCTIONS & set(names))) \
                    or (module.split('.')[0] == 'asyncio' and ('*' in names or ASYNC_PROCESS_FUNCTIONS & set(names))):
                found.append((line, 'from %s import %s' % (module, ', '.join(names))))
            if module in ('', 'importlib') and 'import_module' in names:
                found.append((line, 'from importlib import import_module'))
        elif isinstance(node, ast.Attribute):
            base = module_of(node.value)
            if base == 'os' and node.attr in OS_PROCESS_FUNCTIONS:
                found.append((line, 'os.' + node.attr))
            elif node.attr in ASYNC_PROCESS_FUNCTIONS:
                found.append((line, '.' + node.attr))
            elif base == 'subprocess':
                call = parents.get(node)
                if node.attr == 'run' and isinstance(call, ast.Call) and call.func is node:
                    first = call.args[0] if call.args else None
                    program = first.elts[0].value if isinstance(first, ast.List) and first.elts and isinstance(first.elts[0], ast.Constant) else '<computed>'
                    keywords = sorted(k.arg for k in call.keywords if isinstance(k.value, ast.Constant) and k.value.value is True)
                    found.append((line, 'subprocess.run %s in %s%s, %s' % (program, function_of(node), ' if nt' if windows_only(node) else '', ' '.join(keywords))))
                else:
                    found.append((line, 'subprocess.' + node.attr))
        elif isinstance(node, ast.Name) and aliases.get(node.id) == 'subprocess' and not isinstance(parents.get(node), ast.Attribute):
            found.append((line, 'subprocess (the module as a value)'))
        elif isinstance(node, ast.Call):
            name = module_of(node.func)
            if name in ('__import__', 'eval', 'exec', 'compile', 'builtins.__import__', 'builtins.eval', 'builtins.exec'):
                found.append((line, name + '()'))
            elif name in ('getattr', 'vars', 'setattr', 'delattr') and node.args and isinstance(node.args[0], ast.Name) \
                    and (forbidden_module(module_of(node.args[0])) or module_of(node.args[0]) in ('os', 'asyncio', 'importlib', 'builtins', 'sys')):
                found.append((line, '%s(%s)' % (name, module_of(node.args[0]))))
            elif name.endswith('import_module') or name.endswith('spec_from_file_location') or name.endswith('SourceFileLoader') \
                    or name.endswith('run_path') or name.endswith('run_module'):
                found.append((line, 'load ' + (', '.join(constants(node)) or '<computed>')))
    return sorted(found)


class ProvisioningScriptsCreateNoProcess(unittest.TestCase):
    """application-startup-boundaries requirement 5, read statically: on every path, not only
    the ones the tests above run. provision-runtime.py and provision-model.py can create no
    process, start no server and load only each other; provision-provider.py's only processes
    are its four access-control helper calls, each a blocking subprocess.run (so it returns
    before the script goes on) with `check` and `capture_output`, reached only when
    `os.name == 'nt'`, and it loads only its two sibling scripts. A static scan cannot tell the
    native mode from --container (outside the requirement, design decision 8), so a process
    added to the container mode also changes the list and must be reviewed against the spec."""
    SCRIPTS = Path(__file__).resolve().parent

    def test_the_scanner_finds_each_way_to_start_a_process(self):
        for snippet in ['import subprocess as sp\nsp.Popen(["x"])', 'from subprocess import Popen as P', 'import os\nos.system("x")',
                        'from os import posix_spawn', 'import multiprocessing', 'import asyncio\nasyncio.create_subprocess_exec("x")',
                        'getattr(os, "sys" + "tem")("x")', '__import__("subpro" + "cess")', 'exec("import subprocess")',
                        'import importlib\nimportlib.import_module("sub" + "process")', 'import http.server', 'import webbrowser',
                        'import ctypes', 'import winreg', 'def f(run=subprocess.run): pass', 'os.execv("/bin/x", ["x"])',
                        'import os as o\no.fork()', 'runpy.run_path("other.py")']:
            with self.subTest(snippet=snippet):
                self.assertNotEqual(process_constructs(snippet), [], snippet)
        windows ="import os, subprocess\ndef protect():\n    if os.name == 'nt':\n        subprocess.run(['icacls', 'x'], check=True, capture_output=True)\n    else:\n        subprocess.run(['chmod', 'x'])\n"
        self.assertEqual(process_constructs(windows), [(1, 'import subprocess'), (4, 'subprocess.run icacls in protect if nt, capture_output check'),
                                                       (6, 'subprocess.run chmod in protect, ')])
        for harmless in ['import os\nos.chmod("x", 0o600)\nos.name', 'import json, hashlib, shutil\nshutil.copyfile("a", "b")']:
            self.assertEqual(process_constructs(harmless), [], harmless)

    def test_native_provisioning_scripts_create_no_process_on_any_path(self):
        sources = {name: (self.SCRIPTS / name).read_text() for name in PROVISIONING_SCRIPTS}
        self.assertEqual(process_constructs(sources['provision-runtime.py']), [(25, 'load chiral_gguf_artifacts, provision-model.py')])
        self.assertEqual(process_constructs(sources['provision-model.py']), [])
        provider = [description for _, description in process_constructs(sources['provision-provider.py'])]
        self.assertEqual(provider, [
            'import subprocess',
            'load provision-model',
            'load provision-runtime',
            "subprocess.run whoami in protect if nt, capture_output check text",
            "subprocess.run icacls in protect if nt, capture_output check",
            "subprocess.run icacls in protect if nt, capture_output check",
            "subprocess.run powershell.exe in private_file if nt, capture_output check",
            'subprocess.SubprocessError',
        ])


def compose_services(text):
    """The services of a Compose file in this repository's block style: each service's
    top-level keys, a scalar or inline value as text and a block as its lines. Anchors (`<<`)
    are kept as keys. Only what this repository's file uses is supported; anything else
    fails the parse."""
    lines = [line for line in text.splitlines() if line.strip() and not line.lstrip().startswith('#')]
    services, top, current, key = {}, None, None, None
    for line in lines:
        indent = len(line) - len(line.lstrip(' '))
        stripped = line.strip()
        if indent == 0:
            top = stripped.split(':', 1)[0]
            current = None
            continue
        if top != 'services':
            continue
        if indent == 2:
            match = re.fullmatch(r'([A-Za-z0-9_.-]+):', stripped)
            if not match:
                raise AssertionError('unsupported service line: ' + line)
            current = services.setdefault(match.group(1), {})
        elif indent == 4:
            match = re.fullmatch(r'(<<|[A-Za-z0-9_-]+):\s*(.*)', stripped)
            if not match or current is None:
                raise AssertionError('unsupported service key: ' + line)
            key = match.group(1)
            current[key] = match.group(2) if match.group(2) else []
        elif isinstance(current.get(key), list):
            current[key].append(stripped)
        else:
            raise AssertionError('unsupported continuation: ' + line)
    return services


def dockerfile_stages(text):
    """Each `FROM ... AS name` stage's base and its ENTRYPOINT, CMD and COPY lines."""
    stages, current = {}, None
    for line in text.splitlines():
        match = re.fullmatch(r'FROM\s+(\S+)\s+AS\s+(\S+)', line.strip())
        if match:
            current = stages.setdefault(match.group(2), {'from': match.group(1), 'ENTRYPOINT': None, 'CMD': None, 'COPY': [], 'RUN': []})
            continue
        if current is not None:
            for instruction in ('ENTRYPOINT', 'CMD', 'COPY', 'RUN'):
                if line.startswith(instruction + ' '):
                    value = line[len(instruction) + 1:].strip()
                    if instruction in ('COPY', 'RUN'):
                        current[instruction].append(value)
                    else:
                        current[instruction] = json.loads(value)
    return stages


class ReferenceComposeDeployment(unittest.TestCase):
    """application-startup-boundaries requirement 5, scenario "The Compose deployment has no
    node or miner": the reference Compose deployment and its Dockerfile, read as files. Each
    service runs one of the repository's Chiral binaries from its own Dockerfile stage, or the
    pinned Caddy ingress image, so none is a chain node, a miner or a standalone engine."""
    REPOSITORY = Path(__file__).resolve().parent.parent

    def test_the_compose_services_run_only_chiral_binaries_and_the_pinned_ingress(self):
        services = compose_services((self.REPOSITORY / 'docker-compose.yml').read_text())
        stages = dockerfile_stages((self.REPOSITORY / 'Dockerfile').read_text())
        self.assertEqual(sorted(services), ['chain-rpc', 'ingress', 'provider', 'relay'])
        binaries = {'provider': 'chiral_daemon', 'relay': 'relay_server', 'chain-rpc': 'chiral_rpc_gateway'}
        for name, binary in binaries.items():
            service = services[name]
            with self.subTest(service=name):
                self.assertEqual(service['<<'], '*rust')
                self.assertNotIn('entrypoint', service)
                self.assertRegex(service['image'], r'^chiral-[a-z]+:\$\{RELEASE_ID')
                build = service['build']
                target = re.search(r'target:\s*([A-Za-z0-9_-]+)', build if isinstance(build, str) else ' '.join(build)).group(1)
                self.assertEqual(target, name)
                stage = stages[target]
                self.assertEqual((stage['from'], stage['ENTRYPOINT']), ('runtime', [binary]))
                self.assertEqual(stage['COPY'], ['--from=builder /build/src-tauri/target/release/%s /usr/local/bin/%s' % (binary, binary)])
                self.assertEqual(stage['RUN'], [])
                command = service.get('command')
                if command is not None:
                    # Arguments of the binary only: flags and their values, no other program.
                    arguments = json.loads(re.sub(r'([\[,]\s*)([^",\[\]\s][^,\]]*)', lambda m: m.group(1) + json.dumps(m.group(2).strip()), command))
                    self.assertTrue(arguments[0].startswith('--'), arguments)
                    self.assertTrue(all(arg.startswith(('--', '/', '127.0.0.1:')) for arg in arguments), arguments)
        ingress = services['ingress']
        self.assertEqual(ingress['image'], '${CADDY_IMAGE:?Pinned Caddy digest required}')
        for key in ('build', 'command', 'entrypoint', '<<'):
            self.assertNotIn(key, ingress)
        self.assertEqual(ingress['volumes'], ['- ${DEPLOY_DIR}/Caddyfile:/etc/caddy/Caddyfile:ro', '- ${DEPLOY_DIR}/data/tls:/data',
                                              '- ${DEPLOY_DIR}/data/caddy:/config'])
        # The shared runtime stage installs libraries and certificates only, and copies only the CLI.
        runtime = stages['runtime']
        self.assertEqual(runtime['COPY'], ['--from=builder /build/src-tauri/target/release/chiral /usr/local/bin/chiral'])
        packages = re.search(r'apt-get install -y --no-install-recommends ([^&]+)&&', runtime['RUN'][0]).group(1).split()
        self.assertEqual(packages, ['ca-certificates', 'libssl3', 'libgomp1', 'curl'])
        self.assertEqual({name: stage['ENTRYPOINT'] for name, stage in stages.items() if stage['ENTRYPOINT']},
                         {'provider': ['chiral_daemon'], 'relay': ['relay_server'], 'chain-rpc': ['chiral_rpc_gateway'], 'cli': ['chiral']})


if __name__ == '__main__':
    unittest.main()
