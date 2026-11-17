"""Behavioral boundary tests; synthetic fixtures are not qualification evidence."""
import copy
import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('qualify_provider', Path(__file__).with_name('qualify-provider.py'))
qualifier = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qualifier)


def identity(memory=8188, kind='dedicated', backend='cuda'):
    return {
        'pid': 123, 'engine_url': 'http://127.0.0.1:8000', 'model_id': 'org/model/Q4.gguf',
        'context_length': 4096, 'max_concurrency': 1, 'platform': 'macos' if backend == 'metal' else 'linux',
        'backend': backend, 'memory_kind': kind,
        'devices': [{'id': 'physical-1', 'name': 'Test device', 'memory_mib': memory,
                     'driver_version': 'test-driver', 'engine_device': 'MTL0' if backend == 'metal' else 'CUDA0'}],
        'runtime_sha256': 'a' * 64, 'artifact_manifest_sha256': 'b' * 64,
        'engine_version': 'v0.9.0', 'memory_reserve_mib': 4096 if kind == 'unified' else 256,
        'gpu_layers': 999, 'split_mode': 'none', 'tensor_split': [], 'cache_type_k': 'f16',
        'cache_type_v': 'f16', 'flash_attention': True, 'batch_size': 512, 'ubatch_size': 128,
    }


class RuntimeInspectionTests(unittest.TestCase):
    def inspect(self, observed=None, mutate=None, capabilities=('chat', 'completions'), tier='8gb', managed=True, ready=True):
        observed = copy.deepcopy(observed or identity())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = {'model': {'repo_id': 'org/model'}}
            data = qualifier.canonical(manifest)
            (root / 'chiral-artifact-sha256.json').write_bytes(data)
            observed['artifact_manifest_sha256'] = qualifier.sha(data)
            profile = {field: observed[field] for field in qualifier.OBSERVED_FIELDS}
            profile.update(engine='llama.cpp', hf_repo='org/model', tokenizer_repo='org/model', capacity_tier=tier,
                           policy='warm-generation-v1', output_tokens_per_second=20, arrival_milli_rps=1000)
            model = {'id': observed['model_id'], 'revision': 'c' * 40, 'tokenizer_revision': 'c' * 40,
                     'context_length': 4096, 'max_output_tokens': 256, 'quantization': 'Q4_K_M',
                     'capabilities': list(capabilities), 'serving': profile}
            if mutate:
                mutate(model)
            base = dict(model, serving=None)
            args = SimpleNamespace(admin_url='http://127.0.0.1:9421', admin_key_file='private-file',
                                   engine_url='http://127.0.0.1:8000', artifact_root=directory)
            response = {'models': [{'model_id': model['id'], 'managed': managed, 'ready': ready, 'observation': observed}]}
            provision = SimpleNamespace(validate_artifact=lambda _: (base, 'generation'))
            with patch.object(qualifier, 'provision_provider', return_value=provision), \
                 patch.object(qualifier, 'get_json', return_value=response), \
                 patch.object(qualifier, 'secret', return_value='not-a-real-credential'), \
                 patch.object(qualifier, 'memory_reading', return_value=('device_used', 4000, 4188)), \
                 patch.object(qualifier.platform, 'system', return_value='Darwin' if observed['platform'] == 'macos' else 'Linux'):
                return qualifier.inspect_runtime(args, model)

    def test_eight_gb_tier_uses_observed_physical_capacity(self):
        self.inspect(identity(memory=7500))
        with self.assertRaisesRegex(ValueError, 'capacity tier'):
            self.inspect(identity(memory=7499))
        with self.assertRaisesRegex(ValueError, 'capacity tier'):
            self.inspect(identity(memory=8188), tier='16gb')

    def test_unified_nominal_capacity_and_headroom_are_distinct(self):
        observed = identity(memory=16384, kind='unified', backend='metal')
        self.assertEqual(self.inspect(observed, tier='16gb')['memory_reserve_mib'], 4096)
        observed['memory_reserve_mib'] = 4095
        with self.assertRaisesRegex(ValueError, 'headroom'):
            self.inspect(observed, tier='16gb')

    def test_candidate_claims_never_replace_runtime_observation(self):
        for field, replacement in [('runtime_sha256', 'f' * 64), ('gpu_layers', 20),
                                   ('cache_type_k', 'q8_0'), ('memory_reserve_mib', 512), ('max_concurrency', 2)]:
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'observed runtime differs'):
                self.inspect(mutate=lambda model: model['serving'].__setitem__(field, replacement))

    def test_external_or_dead_process_cannot_qualify(self):
        for managed, ready in [(False, True), (True, False)]:
            with self.subTest(managed=managed, ready=ready), self.assertRaisesRegex(ValueError, 'healthy managed'):
                self.inspect(managed=managed, ready=ready)
        observed = identity()
        observed['engine_url'] = 'http://127.0.0.1:8001'
        with self.assertRaisesRegex(ValueError, 'engine URL'):
            self.inspect(observed)

    def test_generation_capabilities_do_not_require_separate_streaming_flag(self):
        for capabilities in [('chat',), ('completions',), ('chat', 'completions')]:
            self.inspect(capabilities=capabilities)
        for capabilities in [(), ('streaming',), ('tools',), ('chat', 'embeddings')]:
            with self.subTest(capabilities=capabilities), self.assertRaisesRegex(ValueError, 'workload/capabilities'):
                self.inspect(capabilities=capabilities)

    def test_cpu_cannot_be_certified_as_gpu_speed_tier(self):
        with self.assertRaisesRegex(ValueError, 'cannot certify'):
            self.inspect(identity(kind='system', backend='cpu'))


class MemoryObservationTests(unittest.TestCase):
    def test_cuda_reads_selected_physical_device_only(self):
        runtime = identity()
        with patch.object(qualifier, 'command', return_value='physical-1, 8188, 4000, 4188\nother, 24000, 20000, 4000\n'):
            self.assertEqual(qualifier.memory_reading(runtime), ('device_used', 4000, 4188))
        with patch.object(qualifier, 'command', return_value='physical-1, 16000, 4000, 12000\n'):
            with self.assertRaisesRegex(ValueError, 'inventory differs'):
                qualifier.memory_reading(runtime)

    def test_multi_device_available_is_minimum_not_sum(self):
        runtime = identity()
        runtime['devices'].append(dict(runtime['devices'][0], id='physical-2'))
        with patch.object(qualifier, 'command', return_value='physical-1, 8188, 4000, 4188\nphysical-2, 8188, 8000, 188\n'):
            self.assertEqual(qualifier.memory_reading(runtime), ('device_used', 12000, 188))

    def test_mac_system_memory_is_not_reported_as_vram(self):
        runtime = identity(memory=16384, kind='unified', backend='metal')
        with patch.object(qualifier, 'command', side_effect=[str(16384 * 1024 * 1024),
                'Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 262144.\nPages inactive: 100.\n']):
            self.assertEqual(qualifier.memory_reading(runtime), ('system_used', 12288, 4096))

    def test_missing_memory_metric_fails_closed(self):
        with self.assertRaisesRegex(ValueError, 'no trustworthy'):
            qualifier.memory_reading(identity(backend='vulkan'))
        sampler = qualifier.MemorySampler(identity())
        with patch.object(qualifier, 'memory_reading', side_effect=ValueError('counter unavailable')):
            with self.assertRaisesRegex(ValueError, 'observation failed'):
                with sampler:
                    sampler.worker.join(1)


class SamplingTests(unittest.TestCase):
    def test_performance_requires_authoritative_usage_and_exact_clock(self):
        sample = dict(error=False, usage_known=True, prompt_tokens=128, output_tokens=256,
                      ttft_us=100000, decode_us=10000000, elapsed_us=10100000)
        self.assertTrue(qualifier.passes(sample, 20))
        for field, value in [('usage_known', False), ('output_tokens', 255), ('decode_us', 0), ('elapsed_us', 1)]:
            with self.subTest(field=field):
                self.assertFalse(qualifier.passes(dict(sample, **{field: value}), 20))

    def test_chat_workload_uses_actual_template_and_token_ids(self):
        calls = []
        class Response(io.BytesIO):
            pass
        def request(base, path, body, token):
            calls.append((path, body))
            if path == '/apply-template':
                return Response(qualifier.canonical({'prompt': 'rendered:' + body['messages'][0]['content']}))
            self.assertTrue(body['content'].startswith('rendered:'))
            count = 40 + body['content'].count(' a')
            return Response(qualifier.canonical({'tokens': [1] * count}))
        with patch.object(qualifier, 'request', request):
            _, count, ids = qualifier.workload_prompt(SimpleNamespace(engine_url='http://127.0.0.1:8000'), {'id': 'model'}, 'chat', 128, 'private')
        self.assertTrue(120 <= count <= 128)
        self.assertEqual(len(ids), count)
        self.assertTrue(all(body['parse_special'] and body['add_special'] for path, body in calls if path == '/tokenize'))
    def test_engine_stream_requires_single_token_ids_and_terminal_usage(self):
        class Response(io.BytesIO):
            headers = {}
        model = {'id': 'model'}
        args = SimpleNamespace(engine_url='http://127.0.0.1:8000')
        def measure(ids, finish=True, output_tokens=2):
            frames = [{'choices': [{'text': 'x'}], '__verbose': {'tokens': token_ids}} for token_ids in ids]
            frames.append({'choices': [{'text': '', 'finish_reason': 'length'}], '__verbose': {'tokens': []},
                           'usage': {'prompt_tokens': 128, 'completion_tokens': output_tokens}})
            data = b''.join(b'data: ' + qualifier.canonical(frame) + b'\n\n' for frame in frames)
            if finish:
                data += b'data: [DONE]\n\n'
            def request(base, route, body, **kwargs):
                self.assertEqual(body['prompt'], [1] * 128)
                self.assertTrue(body['return_tokens'])
                self.assertEqual(body['n_probs'], 0)
                return Response(data)
            with patch.object(qualifier, 'request', request):
                return qualifier.sample(args, model, 'engine', 'completions', True, 0, 'test-key',
                                        qualifier.Inflight(), ('prompt', 128, [1] * 128))
        self.assertFalse(measure([[10], [11]])['error'])
        self.assertTrue(measure([[10, 11]])['error'])
        self.assertTrue(measure([[], []])['error'])
        self.assertTrue(measure([[10], [11]], finish=False)['error'])
        self.assertTrue(measure([[10], [11]], output_tokens=3)['error'])


    def test_private_endpoints_reject_dns_and_redirect_capable_origins(self):
        for origin in ('http://localhost:8000', 'http://example.com', 'http://127.0.0.1:8000/path', 'http://user@127.0.0.1'):
            with self.subTest(origin=origin), self.assertRaises(ValueError):
                qualifier.private_origin(origin)
        qualifier.private_origin('http://[::1]:8000')


class NativeProvisioningTests(unittest.TestCase):
    def test_generated_secret_permissions_match_native_private_file_contract(self):
        provision = qualifier.provision_provider()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'credential'
            path.write_text('test-only-not-a-real-credential')
            provision.protect(path, 0o400)
            provision.private_file(path)
            if qualifier.os.name == 'nt':
                qualifier.subprocess.run(['icacls', str(path), '/grant', '*S-1-5-18:R'],
                                         check=True, capture_output=True)
            else:
                path.chmod(0o440)
            with self.assertRaises((ValueError, qualifier.subprocess.CalledProcessError)):
                provision.private_file(path)

    def test_runtime_settings_reject_unbound_flags_and_invalid_memory_policy(self):
        provision = qualifier.provision_provider()
        settings = {key: value for key, value in identity().items() if key in provision.RUNTIME_FIELDS}
        settings.update(device_ids=['CUDA0'], startup_timeout_secs=120)
        expected = provision.engine_settings(settings, '/runtime', '/artifact')
        self.assertEqual(expected['runtime_dir'], '/runtime')
        self.assertEqual(expected['artifact_dir'], '/artifact')
        for changes in ({'extra_args': ['--fit', 'on']}, {'cache_type_v': 'q8_0', 'flash_attention': False},
                        {'backend': 'metal', 'device_ids': ['MTL0'], 'memory_reserve_mib': 256},
                        {'batch_size': 8193}, {'device_ids': ['CUDA0', 'CUDA0']}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                provision.engine_settings(dict(settings, **changes), '/runtime', '/artifact')

    def test_native_generation_uses_managed_runtime_and_private_host_paths(self):
        provision = qualifier.provision_provider()
        for system, platform_name in [('Linux', 'linux'), ('Darwin', 'macos'), ('Windows', 'windows')]:
            with self.subTest(platform=platform_name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                model_root = root / 'model'
                runtime_root = root / 'runtime'
                model_root.mkdir()
                runtime_root.mkdir()
                model = {'id': 'org/model/Q4.gguf', 'revision': 'c' * 40, 'tokenizer_revision': 'c' * 40,
                         'context_length': 4096, 'max_output_tokens': 256, 'quantization': 'Q4_K_M',
                         'capabilities': ['completions'], 'serving': None}
                (model_root / 'chiral-artifact-sha256.json').write_text('{}')
                (runtime_root / 'chiral-runtime.json').write_text('{}')
                (root / 'policy.json').write_text(json.dumps({'signature': 'f' * 128}))
                (root / 'provider.key').write_text('1' * 64)
                (root / 'rpc.token').write_text('test-only-' * 8)
                settings = dict(backend='cpu', device_ids=[], memory_reserve_mib=256, gpu_layers=0,
                                split_mode='none', tensor_split=[], cache_type_k='f16', cache_type_v='f16',
                                flash_attention=False, batch_size=512, ubatch_size=128, startup_timeout_secs=120)
                inputs = dict(release_id='test', policy_public_key='a' * 64, version_policy_file=str(root / 'policy.json'),
                              provider_domain='provider.example.com', relay_domain='relay.example.com', rpc_domain='rpc.example.com',
                              acme_email='operator@example.com', platform_wallet='0x' + '2' * 40,
                              provider_key_file=str(root / 'provider.key'), rpc_token_file=str(root / 'rpc.token'),
                              model_dir=str(model_root), runtime_dir=str(runtime_root), runtime=settings,
                              input_wei_per_1k='1', output_wei_per_1k='1', min_funding_wei='1',
                              max_concurrency=1, max_output_tokens=256, context_length=4096, request_timeout_secs=120,
                              provider_p2p_port=4001, relay_p2p_port=4002, bootstrap=[],
                              network={'identity': {'chain_id': 71, 'genesis_hash': provision.ESPACE_GENESIS, 'deployment_id': 'test'},
                                       'rpc_url': 'https://evmtestnet.confluxrpc.com'})
                input_path = root / 'inputs.json'
                input_path.write_text(json.dumps(inputs))
                destination = root / 'deployment'
                with patch.object(provision, 'validate_artifact', return_value=(model, 'generation')), \
                     patch.object(provision, 'validate_runtime', return_value={'platform': platform_name, 'backend': 'cpu', 'engine_version': 'v0.9.0'}), \
                     patch.object(provision, 'private_file'), patch.object(provision, 'protect') as protect, \
                     patch.object(provision.platform, 'system', return_value=system), \
                     patch('sys.argv', ['provision-provider.py', '--inputs', str(input_path), '--output', str(destination), '--calibration']), \
                     patch('sys.stdout', new_callable=io.StringIO):
                    provision.main()
                config = json.loads((destination / 'provider.json').read_text())
                served = config['models'][0]
                # provision-provider.py records canonical (symlink-resolved) model/runtime paths;
                # on macOS the temporary directory itself sits behind the /var -> /private/var symlink.
                self.assertEqual(served['runtime'], dict(settings, runtime_dir=str(runtime_root.resolve(strict=True)),
                                                         artifact_dir=str(model_root.resolve(strict=True))))
                self.assertEqual(served['engine_url'], 'http://127.0.0.1:8000')
                self.assertEqual(served['engine_api_key_file'], str(destination / 'secrets/engine.token'))
                self.assertEqual(config['admin_bind'], '127.0.0.1:9421')
                self.assertEqual(config['dht']['bootstrap'], [])
                self.assertIsNone(served['qualification_evidence_file'])
                self.assertFalse((destination / 'compose.env').exists())
                self.assertTrue(all(call.args[-1] is False for call in protect.call_args_list))


if __name__ == '__main__':
    unittest.main()
