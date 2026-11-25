#!/usr/bin/env python3
"""Generate private managed Strata or explicit legacy llama.cpp provider configurations.

Native Windows/macOS/Linux is the default; --container generates Linux Compose
paths and device access. Both modes use the Rust daemon's pinned runtime launcher.
Inputs include model_dir, runtime_dir, runtime (explicit EngineRuntimeConfig
settings without paths), network, prices, release policy and private key files.
Production generation also requires qualified_model_file and
qualification_evidence_file; --calibration is unpublished loopback-only.
Never replaces an existing destination or starts network services.
"""
import argparse
import importlib
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import stat
import platform
import subprocess
from urllib.parse import urlsplit

UID = 10001
def manifest_at(root, name):
    path = Path(root) / name
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 4 * 1024 * 1024,
            'invalid bounded artifact/runtime metadata')
    return json.loads(path.read_bytes())

def validate_artifact(root):
    manifest = manifest_at(root, 'chiral-artifact-sha256.json')
    if manifest.get('schema') == 'chiral-strata-artifact-v1':
        return importlib.import_module('provision-strata').validate_artifact(root)
    return importlib.import_module('provision-model').validate_artifact(root)

def validate_runtime(root):
    manifest = manifest_at(root, 'chiral-runtime.json')
    if manifest.get('schema') == 'chiral-strata-runtime-v1':
        return importlib.import_module('provision-strata').validate_runtime(root)
    return importlib.import_module('provision-runtime').validate_runtime(root)
RUNTIME_FIELDS = set('backend device_ids memory_reserve_mib gpu_layers split_mode tensor_split cache_type_k cache_type_v flash_attention batch_size ubatch_size startup_timeout_secs'.split())
ESPACE_GENESIS = '0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804'

def require(condition, message):
    if not condition:
        raise ValueError(message)

def protect(path, mode, container=False):
    if os.name == 'nt':
        # Remove inherited access and grant only the current Windows identity.
        identity = subprocess.run(['whoami'], check=True, capture_output=True, text=True).stdout.strip()
        subprocess.run(['icacls', str(path), '/setowner', identity],
                       check=True, capture_output=True)
        subprocess.run(['icacls', str(path), '/inheritance:r', '/grant:r', identity + ':F'],
                       check=True, capture_output=True)
    else:
        path.chmod(mode)
        if container:
            os.chown(path, UID, UID)

def private_file(path):
    if os.name == 'nt':
        script = (
            "$ErrorActionPreference='Stop'; $a=Get-Acl -LiteralPath $args[0]; "
            "$s=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value; "
            "$r=@($a.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])); "
            "if(-not $a.AreAccessRulesProtected -or "
            "$a.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $s -or "
            "$r.Count -ne 1 -or $r[0].AccessControlType -ne 'Allow' -or "
            "$r[0].IdentityReference.Value -ne $s){exit 1}"
        )
        subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', script, str(path)],
                       check=True, capture_output=True)
    else:
        require(stat.S_IMODE(path.stat().st_mode) & 0o077 == 0,
                'secret input must be inaccessible to group/other')

def secret_file(value):
    path = Path(value).absolute()
    require(not path.is_symlink() and path.is_file(), 'secret input must be a regular non-symlink file')
    private_file(path)
    data = path.read_text().strip()
    require(data and not any(c.isspace() for c in data), 'invalid secret input')
    return data

def verify_qualification(model, base, manifest_bytes, evidence_bytes):
    """Bind inputs to evidence; the native provider validates its full workload."""
    require(dict(model, serving=None) == base, 'qualified model differs from immutable artifact')
    profile = model['serving']
    require(isinstance(profile, dict), 'generation requires a qualified serving profile')
    manifest = json.loads(manifest_bytes)
    for field, expected in {
        'hf_repo': manifest['model']['repo_id'],
        'tokenizer_repo': manifest['model']['repo_id'],
        'artifact_manifest_sha256': hashlib.sha256(manifest_bytes).hexdigest(),
        'engine': 'strata' if manifest.get('schema') == 'chiral-strata-artifact-v1' else 'llama.cpp',
    }.items():
        require(profile.get(field) == expected, 'qualified artifact differs: ' + field)
    require(len(evidence_bytes) <= 64 * 1024 * 1024, 'qualification evidence exceeds 64 MiB')
    evidence = json.loads(evidence_bytes)
    qualification = profile['qualification']
    require(hashlib.sha256(evidence_bytes).hexdigest() == qualification['evidence_sha256'],
            'qualification evidence digest differs')
    fingerprint_model = dict(model, serving=dict(profile))
    del fingerprint_model['serving']['qualification']
    fingerprint = hashlib.sha256(json.dumps(fingerprint_model, sort_keys=True,
                                separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()
    require(evidence['schema'] == 'chiral-performance-evidence-v2'
            and evidence['profile_sha256'] == fingerprint
            and evidence['measured_at'] == qualification['measured_at'],
            'qualification evidence describes a different profile')
    return profile

def engine_settings(config, runtime_dir, artifact_dir):
    strata = isinstance(config, dict) and config.get('strata_catalog') is not None
    require(isinstance(config, dict) and set(config) == RUNTIME_FIELDS | ({'strata_catalog'} if strata else set()), 'explicit runtime settings required; no arbitrary engine flags')
    require(config['backend'] in ('cuda', 'metal', 'vulkan', 'cpu'), 'unsupported runtime backend')
    require(isinstance(config['device_ids'], list) and all(isinstance(x, str) and re.fullmatch(r'[A-Za-z]+[0-9]+', x) for x in config['device_ids'])
            and len(set(config['device_ids'])) == len(config['device_ids']), 'invalid explicit engine devices')
    require((config['backend'] == 'cpu') == (not config['device_ids']), 'GPU backend requires explicit devices')
    require((strata and config['gpu_layers'] == 0) or (config['backend'] == 'cpu' and config['gpu_layers'] == 0)
            or (config['backend'] != 'cpu' and type(config['gpu_layers']) is int and config['gpu_layers'] > 0), 'GPU layer allocation must match backend')
    for field, low, high in [('memory_reserve_mib', 256, 2**32), ('gpu_layers', 0, 999),
                             ('batch_size', 1, 8192), ('ubatch_size', 1, 8192), ('startup_timeout_secs', 1, 3600)]:
        require(type(config[field]) is int and low <= config[field] <= high, 'invalid runtime ' + field)
    require(config['backend'] != 'metal' or config['memory_reserve_mib'] >= 4096, 'unified memory requires at least 4096 MiB reserve')
    require(config['ubatch_size'] <= config['batch_size'], 'ubatch_size exceeds batch_size')
    require(type(config['flash_attention']) is bool and config['split_mode'] in ('none', 'layer', 'row'), 'invalid execution settings')
    require(isinstance(config['tensor_split'], list) and all(type(x) is int and x > 0 for x in config['tensor_split']), 'invalid tensor split')
    require((len(config['device_ids']) <= 1 and not config['tensor_split'] and config['split_mode'] == 'none')
            or (len(config['device_ids']) > 1 and len(config['tensor_split']) == len(config['device_ids']) and config['split_mode'] != 'none'), 'split must match selected devices')
    require(all(config[field] in ('f32', 'f16', 'bf16', 'q8_0', 'q4_0', 'q4_1', 'q5_0', 'q5_1', 'iq4_nl') for field in ('cache_type_k', 'cache_type_v')), 'unsupported KV cache type')
    require(config['cache_type_v'] in ('f32', 'f16', 'bf16') or config['flash_attention'], 'quantized V cache requires flash attention')
    if strata:
        importlib.import_module('provision-strata').entry(config['strata_catalog'])
        require(config['backend'] in ('cpu', 'cuda', 'metal') and len(config['device_ids']) <= 1 and config['gpu_layers'] == 0
                and config['split_mode'] == 'none' and not config['tensor_split']
                and config['cache_type_k'] == config['cache_type_v'] == 'f16'
                and config['flash_attention'] is False and config['batch_size'] == config['ubatch_size'] == 512
                and config['memory_reserve_mib'] >= (256 if config['backend'] == 'cuda' else 4096)
                and config['startup_timeout_secs'] <= 1800, 'invalid curated Strata execution recipe')
    return dict(config, runtime_dir=str(runtime_dir), artifact_dir=str(artifact_dir))

def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--container', action='store_true', help='Linux Compose instead of native paths')
    parser.add_argument('--calibration', action='store_true',
                        help='private unqualified deployment for collecting real measurements')
    args = parser.parse_args()
    require(not args.container or (platform.system() == 'Linux' and os.geteuid() == 0),
            'container provisioning requires Linux root for isolated UID10001 ownership')
    c = json.loads(args.inputs.read_text())
    required = set('release_id policy_public_key version_policy_file provider_domain relay_domain rpc_domain acme_email network platform_wallet provider_key_file rpc_token_file model_dir runtime_dir runtime input_wei_per_1k output_wei_per_1k min_funding_wei max_concurrency max_output_tokens context_length request_timeout_secs provider_p2p_port relay_p2p_port bootstrap'.split())
    if args.container:
        required |= {'rust_image', 'runtime_image', 'caddy_image', 'container_device_ids'}
    require(required <= c.keys(), 'missing input fields: ' + ', '.join(sorted(required - c.keys())))
    require(set(c) <= required | {'upstream_rpc_token_file', 'qualified_model_file', 'qualification_evidence_file'}, 'unknown input fields')
    require(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,63}', c['release_id']), 'invalid release_id')
    require(re.fullmatch(r'[0-9a-fA-F]{64}', c['policy_public_key']) and int(c['policy_public_key'], 16), 'nonzero release policy key required')
    if args.container:
        for field in ['rust_image', 'runtime_image', 'caddy_image']:
            require(re.fullmatch(r'[A-Za-z0-9./:_-]+@sha256:[0-9a-f]{64}', c[field]), field + ' must use an immutable digest')
    for field in ['provider_domain', 'relay_domain', 'rpc_domain']:
        require(re.fullmatch(r'(?=.{1,253}$)[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?', c[field]) and '.' in c[field] and '..' not in c[field], 'invalid public DNS name')
        try:
            ipaddress.ip_address(c[field])
        except ValueError:
            pass
        else:
            raise ValueError('public ingress requires DNS names')
    require(len({c['provider_domain'], c['relay_domain'], c['rpc_domain']}) == 3, 'ingress DNS names must be distinct')
    require(re.fullmatch(r'[A-Za-z0-9._+%-]+@[A-Za-z0-9.-]+', c['acme_email']), 'invalid ACME email')
    require(re.fullmatch(r'0x[0-9a-fA-F]{40}', c['platform_wallet']) and int(c['platform_wallet'], 16), 'platform wallet must be nonzero')
    for field in ['input_wei_per_1k', 'output_wei_per_1k', 'min_funding_wei']:
        require(isinstance(c[field], str) and re.fullmatch(r'0|[1-9][0-9]*', c[field]) and int(c[field]) < 2**128, field + ' must be a u128 decimal string')
    require(int(c['min_funding_wei']) > 0 and (int(c['input_wei_per_1k']) > 0 or int(c['output_wei_per_1k']) > 0), 'paid reference deployment requires nonzero funding and prices')
    for field, low, high in [('max_concurrency', 1, 256), ('context_length', 128, 32768), ('max_output_tokens', 0, 32768), ('request_timeout_secs', 1, 3600), ('provider_p2p_port', 1024, 65535), ('relay_p2p_port', 1024, 65535)]:
        require(type(c[field]) is int and low <= c[field] <= high, 'invalid ' + field)
    require(c['max_output_tokens'] < c['context_length'], 'output limit must leave room for input')
    require(c['provider_p2p_port'] != c['relay_p2p_port'], 'P2P ports must differ')
    runtime_dir = Path(c['runtime_dir']).resolve(strict=True)
    runtime_manifest = validate_runtime(runtime_dir)
    require(runtime_manifest['platform'] == {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}[platform.system()],
            'runtime must match the provisioning host platform')
    require(runtime_manifest['backend'] == c['runtime']['backend'], 'runtime backend mismatch')
    runtime_hash = hashlib.sha256((runtime_dir / 'chiral-runtime.json').read_bytes()).hexdigest()
    require(isinstance(c['bootstrap'], list) and all(isinstance(x, str) and x.startswith('/') and '/p2p/' in x for x in c['bootstrap']),
            'bootstrap entries must be explicit peer multiaddresses')
    network = c['network']
    require(set(network) <= {'identity', 'rpc_url', 'rpc_bearer_file', 'faucet_url', 'explorer_url', 'policy_url'}, 'unknown network fields')
    identity = network['identity']
    require(set(identity) == {'chain_id', 'genesis_hash', 'deployment_id'}, 'full network identity required')
    require(type(identity['chain_id']) is int and identity['chain_id'] == 71
            and identity['genesis_hash'] == ESPACE_GENESIS, 'only the pinned Conflux eSpace testnet is approved')
    require(re.fullmatch(r'[A-Za-z0-9_-]{1,128}', identity['deployment_id']), 'deployment ID must be 1..128 ASCII letters, digits, hyphens or underscores')
    upstream = urlsplit(network['rpc_url'])
    require(upstream.hostname and not upstream.username and not upstream.password and not upstream.query and not upstream.fragment and (upstream.scheme == 'https' or (upstream.scheme == 'http' and upstream.hostname in ['127.0.0.1', '::1'])), 'upstream requires credential-free HTTPS or literal loopback HTTP')
    require(upstream.hostname not in [c['rpc_domain'], c['provider_domain'], c['relay_domain']], 'upstream RPC cannot loop back to this deployment')
    require(not network.get('rpc_bearer_file'), 'use upstream_rpc_token_file instead of embedding container paths')
    key = secret_file(c['provider_key_file'])
    require(re.fullmatch(r'(0x)?[0-9a-fA-F]{64}', key) and 0 < int(key, 16) < 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141, 'invalid provider secp256k1 key')
    rpc_token = secret_file(c['rpc_token_file'])
    require(len(rpc_token) >= 32, 'RPC bearer must contain at least 32 characters')
    upstream_rpc_token = secret_file(c['upstream_rpc_token_file']) if c.get('upstream_rpc_token_file') else None
    # The gateway refuses to start when its inbound bearer equals its upstream bearer.
    require(upstream_rpc_token != rpc_token, 'the inbound RPC bearer must differ from the upstream RPC bearer')
    policy = json.loads(Path(c['version_policy_file']).read_text())
    require(re.fullmatch(r'[0-9a-fA-F]{128}', policy.get('signature', '')), 'signed release policy required; relay verifies the signature at startup')
    model_dir = Path(c['model_dir']).resolve(strict=True)
    model, task = validate_artifact(model_dir)
    manifest_bytes = (model_dir / 'chiral-artifact-sha256.json').read_bytes()
    manifest = json.loads(manifest_bytes)
    strata = manifest.get('schema') == 'chiral-strata-artifact-v1'
    require(strata == (runtime_manifest.get('schema') == 'chiral-strata-runtime-v1')
            and (not strata or (c['runtime'].get('strata_catalog') == manifest['catalog_id'] and c['max_concurrency'] == 1))
            and (strata or c['runtime'].get('strata_catalog') is None), 'model/runtime/catalog selection differs')
    require(model['context_length'] == c['context_length']
            and model['max_output_tokens'] == c['max_output_tokens'], 'model limits differ from provisioned artifact')
    evidence_bytes = None
    if task == 'embeddings':
        require(c['max_output_tokens'] == 0 and c['output_wei_per_1k'] == '0', 'embeddings requires zero output limit and output price')
    else:
        require(c['max_output_tokens'] > 0, 'generation requires a positive output limit')
    if task == 'generation' and not args.calibration:
        require(c.get('qualified_model_file') and c.get('qualification_evidence_file'),
                'production generation requires measured qualification; use --calibration to collect evidence privately')
        qualified = json.loads(Path(c['qualified_model_file']).read_text())
        evidence_bytes = Path(c['qualification_evidence_file']).read_bytes()
        profile = verify_qualification(qualified, model, manifest_bytes, evidence_bytes)
        expected_settings = engine_settings(c['runtime'], runtime_dir, model_dir)
        for field in RUNTIME_FIELDS - {'device_ids', 'startup_timeout_secs'}:
            require(profile.get(field) == expected_settings[field], 'deployment differs from qualification: ' + field)
        require(profile['runtime_sha256'] == runtime_hash and profile['platform'] == runtime_manifest['platform']
                and profile['engine_version'] == runtime_manifest['engine_version']
                and profile['max_concurrency'] == c['max_concurrency'], 'qualified runtime identity differs')
        require([d['engine_device'] for d in profile['devices']] == c['runtime']['device_ids'], 'qualified devices differ')
        model = qualified
    else:
        require(not c.get('qualified_model_file') and not c.get('qualification_evidence_file'),
                'calibration and embedding deployments must not claim generation qualification')
    destination = args.output.absolute()
    require(not destination.exists(), 'destination exists; refusing to replace keys or ledger state')
    if args.container:
        require(all(ch not in str(destination) + str(model_dir) + str(runtime_dir) for ch in '\n\r$ #:\\'), 'container paths must be Compose-safe absolute Linux paths')
    os.umask(0o077)
    destination.mkdir(parents=True, mode=0o750)
    protect(destination, 0o750, args.container)
    def mapped(relative, container_path):
        return container_path if args.container else str(destination / relative)
    runtime = engine_settings(c['runtime'], '/runtime' if args.container else runtime_dir, '/model' if args.container else model_dir)
    def save(name, value, secret=False):
        path = destination / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value if isinstance(value, str) else json.dumps(value, indent=2) + '\n')
        protect(path, 0o400 if secret else 0o440, args.container)
    save('version-policy.json', policy)
    save('secrets/provider.key', key + '\n', True)
    save('secrets/rpc.token', rpc_token + '\n', True)
    save('secrets/admin.token', secrets.token_urlsafe(48) + '\n', True)
    save('secrets/engine.token', secrets.token_urlsafe(48) + '\n', True)
    upstream_network = dict(network)
    upstream_network['rpc_bearer_file'] = None
    save('secrets/upstream-rpc.token', upstream_rpc_token + '\n' if upstream_rpc_token is not None else '', True)
    if upstream_rpc_token is not None:
        upstream_network['rpc_bearer_file'] = mapped('secrets/upstream-rpc.token', '/run/secrets/upstream-rpc.token')
    save('rpc-gateway.json', upstream_network)
    public_network = dict(network, rpc_url='http://127.0.0.1:8547/api/chain/rpc', rpc_bearer_file=mapped('secrets/rpc.token', '/run/secrets/rpc.token'),
                          policy_url=None if args.calibration else 'https://' + c['relay_domain'] + '/api/version-policy')
    dht = lambda port: {'listen': '/ip4/' + ('127.0.0.1' if args.calibration else '0.0.0.0') + '/tcp/' + str(port),
                        'bootstrap': [] if args.calibration else c['bootstrap'], 'identity_file': mapped('data/provider/peer.key', '/var/lib/chiral/peer.key')}
    qualification_dir = destination / 'qualification'
    qualification_dir.mkdir(mode=0o750)
    protect(qualification_dir, 0o750, args.container)
    if evidence_bytes is not None:
        save('qualification/evidence.json', evidence_bytes.decode('utf-8'))
    save('provider.json', {
        'network': public_network, 'data_dir': mapped('data/provider', '/var/lib/chiral'),
        'signing_key_file': mapped('secrets/provider.key', '/run/secrets/provider.key'), 'admin_token_file': mapped('secrets/admin.token', '/run/secrets/admin.token'),
        'public_endpoint': 'http://127.0.0.1:9419' if args.calibration else 'https://' + c['provider_domain'],
        'public_bind': '127.0.0.1:9419', 'admin_bind': '127.0.0.1:9421',
        'platform_wallet': c['platform_wallet'], 'fee_bps': 50, 'min_funding_wei': c['min_funding_wei'],
        'models': [{'model': model, 'price': {'input_wei_per_1k': c['input_wei_per_1k'], 'output_wei_per_1k': c['output_wei_per_1k']},
                    'engine_url': 'http://127.0.0.1:8000', 'engine_api_key_file': mapped('secrets/engine.token', '/run/secrets/engine.token'), 'runtime': runtime,
                    'max_concurrency': c['max_concurrency'], 'request_timeout_secs': c['request_timeout_secs'],
                    'qualification_evidence_file': mapped('qualification/evidence.json', '/etc/chiral/qualification/evidence.json') if evidence_bytes is not None else None}],
        'dht': dht(c['provider_p2p_port']), 'development': args.calibration,
    })
    relay_dht = dht(c['relay_p2p_port'])
    relay_dht['identity_file'] = mapped('data/relay/peer.key', '/var/lib/chiral/peer.key')
    save('relay.json', {'network': public_network, 'data_dir': mapped('data/relay', '/var/lib/chiral'), 'bind': '127.0.0.1:8081', 'dht': relay_dht, 'development': args.calibration, 'version_policy_file': mapped('version-policy.json', '/etc/chiral/version-policy.json')})
    if args.container:
        require(runtime['backend'] in ('cuda', 'cpu'), 'reference container supports CUDA or development CPU; native Vulkan/Metal needs no container')
        devices = c['container_device_ids']
        require(isinstance(devices, list) and len(devices) == len(runtime['device_ids'])
                and len(set(devices)) == len(devices) and all(re.fullmatch(r'GPU-[0-9a-fA-F-]{36}', d) for d in devices), 'explicit physical CUDA UUIDs required')
        if model.get('serving'):
            require(devices == [d['id'] for d in model['serving']['devices']], 'container devices differ from qualification')
        reservation = {'deploy': {'resources': {'reservations': {'devices': [
            {'driver': 'nvidia', 'device_ids': devices, 'capabilities': ['gpu']},
        ]}}}} if devices else {}
        save('provider-devices.json', {'services': {'provider': reservation}})
    save('Caddyfile', '{\n admin off\n}\nhttp://127.0.0.1:9443 {\n respond 503\n}\n' if args.calibration else '''{\n admin off\n email %s\n}\n%s {\n @public path /v1/* /healthz /readyz /api/version-policy\n handle @public {\n  reverse_proxy 127.0.0.1:9419 {\n   flush_interval -1\n  }\n }\n respond 404\n}\n%s {\n @public path /api/offers /api/offers/* /api/ratings/* /api/version-policy /api/health /api/ready\n handle @public {\n  reverse_proxy 127.0.0.1:8081\n }\n respond 404\n}\n%s {\n @rpc path /api/chain/rpc\n handle @rpc {\n  reverse_proxy 127.0.0.1:8547\n }\n respond 404\n}\n''' % (c['acme_email'], c['provider_domain'], c['relay_domain'], c['rpc_domain']))
    if args.container:
        env = {key.upper(): c[key] for key in ['release_id', 'rust_image', 'runtime_image', 'caddy_image']}
        env.update(DEPLOY_DIR=str(destination), MODEL_DIR=str(model_dir), LLAMA_RUNTIME_DIR=str(runtime_dir), CHIRAL_POLICY_PUBLIC_KEY=c['policy_public_key'],
                   COMPOSE_PROFILES='' if args.calibration else 'public')
        save('compose.env', ''.join(f'{key}={value}\n' for key, value in env.items()))
    for name in ['provider', 'relay', 'rpc', 'tls', 'caddy']:
        path = destination / 'data' / name
        path.mkdir(parents=True, mode=0o700)
        protect(path, 0o700, args.container)
    for path in [destination, destination / 'secrets', destination / 'data']:
        protect(path, 0o750, args.container)
    print('Deployment configuration created; no services started. Keep configuration and secrets private.')

if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError, TypeError, subprocess.SubprocessError) as error:
        raise SystemExit('Provisioning refused: ' + str(error))
