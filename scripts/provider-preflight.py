#!/usr/bin/env python3
"""Check native Windows/macOS/Linux or Linux Compose provider prerequisites.
Never downloads, starts or stops services. Readiness is not paid qualification.
"""
import argparse
import importlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import urllib.error
import urllib.request

provision = importlib.import_module('provision-provider')
qualifier = importlib.import_module('qualify-provider')


def run(command):
    return subprocess.run(command, check=True, capture_output=True, text=True, timeout=30).stdout


def check_rpc_gateway_bearer(base='http://127.0.0.1:8547'):
    """The chain RPC gateway must refuse a JSON-RPC request that lacks its inbound bearer.
    TLS ingress forwards public requests to this loopback port, so loopback is not private."""
    body = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': 'eth_chainId', 'params': []}).encode()
    request = urllib.request.Request(base + '/api/chain/rpc', body, {'Content-Type': 'application/json'}, method='POST')
    try:
        with qualifier.OPENER.open(request, timeout=10) as response:
            status = response.status
    except urllib.error.HTTPError as error:
        status = error.code
    if status != 401:
        raise RuntimeError('chain RPC gateway answered a request without its bearer with HTTP %d, not 401' % status)


PRIVATE_ROUTES = ('/admin/status', '/admin/runtime', '/tokenize', '/apply-template', '/metrics', '/api/headless/wallet')


def status(url):
    """The HTTP status of `GET url`, through a client without proxies or redirects."""
    try:
        with qualifier.OPENER.open(url, timeout=10) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code


def check_private_routes(public_endpoint):
    """Provider ingress must not reach a private or retired route: each must be refused
    (401, 403, 404 or 405) at the public endpoint. A redirect is not followed and fails."""
    for path in PRIVATE_ROUTES:
        if status(public_endpoint + path) not in (401, 403, 404, 405):
            raise RuntimeError('private or retired route exposed: ' + path)


def live_runtime_observation(rows):
    """The one managed engine's launcher observation from `GET /admin/runtime`. That route
    reports process liveness as `alive` (null for an operator-owned engine) and keeps readiness
    separate; readiness is read from `/readyz` below, never from the process observation."""
    if len(rows) != 1 or rows[0].get('managed') is not True or rows[0].get('alive') is not True or not rows[0].get('observation'):
        raise RuntimeError('managed runtime not alive/observed')
    return rows[0]['observation']


def check_listeners():
    private_ports = {8000, 9419, 9421, 8547, 8081}
    host = platform.system()
    if host == 'Linux':
        endpoints = [line.split()[3] for line in run(['ss', '-H', '-ltn']).splitlines()]
    elif host == 'Darwin':
        endpoints = [line[1:] for line in run(['lsof', '-nP', '-iTCP', '-sTCP:LISTEN', '-Fn']).splitlines() if line.startswith('n')]
    elif host == 'Windows':
        rows = json.loads(run(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command',
                              'ConvertTo-Json -Compress -InputObject @(Get-NetTCPConnection -State Listen | Select-Object LocalAddress,LocalPort)']))
        endpoints = [str(row['LocalAddress']) + ':' + str(row['LocalPort']) for row in rows]
    else:
        raise RuntimeError('unsupported native provider platform')
    for endpoint in endpoints:
        address, port = endpoint.rsplit(':', 1)
        if int(port) in private_ports and address not in ('127.0.0.1', '::1', '[::1]'):
            raise RuntimeError('private service bound to non-loopback address')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--deployment', type=Path, required=True)
    parser.add_argument('--model-dir', type=Path, required=True)
    parser.add_argument('--runtime-dir', type=Path, required=True)
    parser.add_argument('--compose', type=Path, help='Linux Compose deployment; omit for native')
    parser.add_argument('--daemon', default='chiral_daemon', help='native daemon executable')
    parser.add_argument('--running', action='store_true')
    parser.add_argument('--calibration', action='store_true')
    args = parser.parse_args()
    host = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}.get(platform.system())
    if host is None:
        raise RuntimeError('unsupported native provider platform')
    resolved = None
    if args.compose:
        if host != 'linux' or not shutil.which('docker'):
            raise RuntimeError('Compose option requires Linux and Docker')
        run(['docker', 'info'])
        compose = ['docker', 'compose', '--env-file', str(args.deployment / 'compose.env'), '-f', str(args.compose)]
        resolved = json.loads(run(compose + ['config', '--format', 'json']))
        service = resolved['services']['provider']
        if not service.get('read_only') or service.get('network_mode') != 'host':
            raise RuntimeError('container must use immutable files and explicit host-loopback bindings')
    elif not shutil.which(args.daemon):
        raise RuntimeError('native daemon executable is unavailable')
    config = json.loads((args.deployment / 'provider.json').read_text())
    if config['development'] != args.calibration or config['public_bind'] != '127.0.0.1:9419' or config['admin_bind'] != '127.0.0.1:9421':
        raise RuntimeError('listener isolation or calibration mode differs')
    identity = config['network']['identity']
    if identity['chain_id'] != 71 or identity['genesis_hash'] != provision.ESPACE_GENESIS:
        raise RuntimeError('deployment is not the approved native Conflux eSpace testnet')
    if args.calibration and (config['public_endpoint'] != 'http://127.0.0.1:9419' or config['dht']['bootstrap'] or not config['dht']['listen'].startswith('/ip4/127.0.0.1/')):
        raise RuntimeError('calibration must remain loopback-only and unpublished')
    for path in (args.deployment / 'secrets').iterdir():
        if path.is_symlink() or not path.is_file():
            raise RuntimeError('secret must be a non-symlink regular file')
        provision.private_file(path)
    base, task = provision.validate_artifact(args.model_dir)
    runtime_manifest = provision.validate_runtime(args.runtime_dir)
    if runtime_manifest['platform'] != host:
        raise RuntimeError('runtime platform differs from native host')
    if len(config['models']) != 1:
        raise RuntimeError('reference deployment requires one pinned model')
    served = config['models'][0]
    advertised = served['model']
    runtime = served.get('runtime')
    if not runtime:
        raise RuntimeError('reference deployment requires Rust-managed runtime')
    settings = {key: runtime[key] for key in provision.RUNTIME_FIELDS}
    model_path = '/model' if args.compose else str(args.model_dir.resolve())
    runtime_path = '/runtime' if args.compose else str(args.runtime_dir.resolve())
    if runtime != provision.engine_settings(settings, runtime_path, model_path) or runtime_manifest['backend'] != runtime['backend']:
        raise RuntimeError('managed runtime paths/backend/settings differ from verified artifacts')
    if resolved:
        mounts = {entry['target']: entry for entry in resolved['services']['provider']['volumes']}
        for target, source in [('/model', args.model_dir), ('/runtime', args.runtime_dir)]:
            entry = mounts.get(target, {})
            if not entry.get('read_only') or Path(entry.get('source', '')).resolve() != source.resolve():
                raise RuntimeError('model/runtime mounts must be exact and read-only')
    manifest_bytes = (args.model_dir / 'chiral-artifact-sha256.json').read_bytes()
    runtime_hash = qualifier.sha((args.runtime_dir / 'chiral-runtime.json').read_bytes())
    profile = advertised.get('serving')
    if task == 'generation' and not args.calibration:
        expected_evidence = '/etc/chiral/qualification/evidence.json' if args.compose else str((args.deployment / 'qualification/evidence.json').resolve())
        if served.get('qualification_evidence_file') != expected_evidence:
            raise RuntimeError('qualification evidence path differs')
        profile = provision.verify_qualification(advertised, base, manifest_bytes, (args.deployment / 'qualification/evidence.json').read_bytes())
        for field in provision.RUNTIME_FIELDS - {'device_ids', 'startup_timeout_secs'}:
            if profile[field] != runtime[field]:
                raise RuntimeError('runtime settings differ from qualification: ' + field)
        if (profile['runtime_sha256'] != runtime_hash or profile['platform'] != host
                or profile['engine_version'] != runtime_manifest['engine_version']
                or profile['max_concurrency'] != served['max_concurrency']
                or [device['engine_device'] for device in profile['devices']] != runtime['device_ids']):
            raise RuntimeError('runtime identity differs from qualification')
        if resolved and runtime['backend'] == 'cuda':
            reservations = resolved['services']['provider']['deploy']['resources']['reservations']['devices']
            if len(reservations) != 1 or reservations[0]['device_ids'] != [d['id'] for d in profile['devices']]:
                raise RuntimeError('container physical devices differ from qualification')
    elif advertised != base or served.get('qualification_evidence_file'):
        raise RuntimeError('calibration/embedding metadata must not claim generation qualification')
    if task == 'embeddings' and served['price']['output_wei_per_1k'] != '0':
        raise RuntimeError('embedding output token price must be zero')
    expected_key = '/run/secrets/engine.token' if args.compose else str((args.deployment / 'secrets/engine.token').resolve())
    if served['engine_url'] != 'http://127.0.0.1:8000' or served['engine_api_key_file'] != expected_key:
        raise RuntimeError('private engine endpoint/token path differs')
    print('Verified immutable model and runtime inventories; capabilities: ' + ', '.join(base['capabilities']))
    if args.running:
        admin_url = 'http://' + config['admin_bind']
        admin_token = provision.secret_file(args.deployment / 'secrets/admin.token')
        observed = live_runtime_observation(qualifier.get_json(admin_url, '/admin/runtime', admin_token)['models'])
        expected = {field: runtime[field] for field in provision.RUNTIME_FIELDS - {'device_ids', 'startup_timeout_secs'}}
        expected.update(model_id=base['id'], engine_url=served['engine_url'], context_length=base['context_length'], max_concurrency=served['max_concurrency'], runtime_sha256=runtime_hash, artifact_manifest_sha256=qualifier.sha(manifest_bytes), platform=host, engine_version=runtime_manifest['engine_version'])
        if any(observed.get(field) != value for field, value in expected.items()) or [d['engine_device'] for d in observed['devices']] != runtime['device_ids']:
            raise RuntimeError('live runtime differs from validated configuration')
        if profile:
            qualifier.inspect_runtime(argparse.Namespace(admin_url=admin_url, admin_key_file=args.deployment / 'secrets/admin.token', engine_url=served['engine_url'], artifact_root=args.model_dir), advertised)
        for path in ('/healthz', '/readyz'):
            if status(config['public_endpoint'] + path) != 200:
                raise RuntimeError('provider is not healthy/ready')
        for url in ('http://127.0.0.1:8081/api/health', 'http://127.0.0.1:8081/api/ready', 'http://127.0.0.1:8547/healthz', 'http://127.0.0.1:8547/readyz'):
            if status(url) != 200:
                raise RuntimeError('relay or chain readiness unavailable')
        check_rpc_gateway_bearer()
        check_private_routes(config['public_endpoint'])
        check_listeners()
    print('Preflight prerequisites satisfied; paid inference and release qualification not exercised.')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print('Provider preflight failed: ' + str(error), file=sys.stderr)
        sys.exit(1)
