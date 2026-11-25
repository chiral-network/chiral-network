#!/usr/bin/env python3
"""Install only Chiral's pinned Strata catalog/runtime. Never starts a server.

Runtime packaging accepts an explicitly built native engine and a self-contained
Python distribution with regex 2026.9.10, Jinja2 3.1.6 and MarkupSafe 3.0.3 installed.
The whole dependency tree is inventoried; system Python/venv symlinks are refused.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import urllib.parse
import urllib.request

sys.path.insert(0, str(Path(__file__).parent / 'strata'))
from common import *

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM_FILES = read_json(ROOT / 'dev/strata/upstream-files.json')


class DownloadRedirects(urllib.request.HTTPRedirectHandler):
    max_redirections = 5
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        u = urllib.parse.urlsplit(newurl)
        host = u.hostname or ''
        require(u.scheme == 'https' and not u.username and not u.password
                and (host == 'huggingface.co' or host.endswith('.huggingface.co')
                     or host.endswith('.hf.co') or host.endswith('.xethub.hf.co')), 'unapproved model redirect')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def download(url, target, expected):
    require(shutil.disk_usage(target.parent).free > expected['size_bytes'] + 256 * 1024**2,
            'insufficient disk space for selected artifact')
    with urllib.request.build_opener(urllib.request.ProxyHandler({}), DownloadRedirects).open(url, timeout=30) as response, target.open('xb') as out:
        remaining = expected['size_bytes']
        while remaining:
            block = response.read(min(1024 * 1024, remaining))
            require(block, 'truncated download')
            out.write(block)
            remaining -= len(block)
        require(not response.read(1), 'download exceeds pinned size')
    require(digest(target) == expected['sha256'], 'download checksum mismatch')


def install_model(name, output, accept_license, source=None):
    e = entry(name)
    require(accept_license == e['license'], 'acknowledge the selected model license explicitly')
    output = Path(output)
    require(not output.exists(), 'refusing to replace an existing artifact')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.strata-install-', dir=output.parent) as temporary:
        stage = Path(temporary) / 'model'
        stage.mkdir()
        for name, expected in e['files'].items():
            dst = stage / relative(name)
            if source:
                src = Path(source) / name
                require(src.is_file() and not src.is_symlink(), 'missing/unsafe source artifact')
                require(src.stat().st_size == expected['size_bytes'] and digest(src) == expected['sha256'], 'source artifact checksum mismatch')
                shutil.copyfile(src, dst)
            else:
                download(f"https://huggingface.co/{e['repo_id']}/resolve/{e['revision']}/{urllib.parse.quote(name)}", dst, expected)
        (stage / 'chiral-model.json').write_bytes(canonical(model_spec(e)))
        (stage / 'chiral-artifact-sha256.json').write_bytes(canonical(artifact_manifest(e)))
        validate_artifact(stage)
        stage.rename(output)


def copy_tree(source, dest):
    source = Path(source).resolve()
    require(source.is_dir(), 'missing runtime directory')
    count = 0
    for p in source.rglob('*'):
        # A self-contained distribution can use internal aliases. Materialize
        # those, but never pull a system interpreter/library into the inventory.
        resolved = p.resolve()
        require(resolved.is_relative_to(source), 'runtime dependency escapes its package')
        if p.is_dir():
            require(not p.is_symlink(), 'runtime directory symlinks are forbidden')
            continue
        require(p.is_file(), 'runtime dependency is not a regular file')
        if '__pycache__' in p.parts or p.suffix == '.pyc':
            continue
        count += 1
        require(count <= 7500, 'runtime package exceeds file limit')
        target = dest / p.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(resolved, target)


def install_runtime(a):
    output, source = Path(a.output), Path(a.source_dir)
    require(not output.exists(), 'refusing to replace an existing runtime')
    for name, checksum in UPSTREAM_FILES.items():
        require(digest(source / name) == checksum, 'upstream source differs from reviewed commit: ' + name)
    engine = Path(a.engine_file)
    require(engine.is_file() and not engine.is_symlink() and digest(engine) == a.engine_sha256, 'native engine checksum mismatch')
    # Reuse native-format checks, never execute the candidate to inspect it.
    spec = importlib.util.spec_from_file_location('legacy_runtime', ROOT / 'scripts/provision-runtime.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.binary_identity(engine, a.platform, a.architecture, 'executable')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.strata-runtime-', dir=output.parent) as temporary:
        stage = Path(temporary) / 'runtime'
        stage.mkdir()
        copy_tree(a.python_dir, stage / 'python')
        python = 'python/python.exe' if a.platform == 'windows' else 'python/bin/python3'
        require((stage / python).is_file(), 'self-contained Python entry point is missing')
        module.binary_identity(stage / python, a.platform, a.architecture, 'executable')
        native = 'strata-inkling.exe' if a.platform == 'windows' else 'strata-inkling'
        shutil.copy2(engine, stage / native)
        if a.engine_libraries:
            copy_tree(a.engine_libraries, stage / 'lib')
        for name in UPSTREAM_FILES:
            dest = stage / 'upstream' / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source / name, dest)
        for name in ('common.py', 'server.py'):
            shutil.copy2(ROOT / 'scripts/strata' / name, stage / name)
        shutil.copy2(ROOT / 'dev/strata/catalog.json', stage / 'catalog.json')
        files = {p.relative_to(stage).as_posix(): {'size_bytes': p.stat().st_size, 'sha256': digest(p)}
                 for p in stage.rglob('*') if p.is_file()}
        manifest = {'schema': RUNTIME_SCHEMA, 'engine_version': ENGINE_VERSION, 'source_revision': SOURCE_REVISION,
                    'platform': a.platform, 'architecture': a.architecture, 'backend': a.backend,
                    'executable': native, 'python': python, 'files': files}
        (stage / 'chiral-runtime.json').write_bytes(canonical(manifest))
        validate_runtime(stage, a.platform, a.architecture, a.backend)
        stage.rename(output)


def validate_runtime(root, platform=None, architecture=None, backend=None):
    root = Path(root)
    m = read_json(root / 'chiral-runtime.json')
    require(m['schema'] == RUNTIME_SCHEMA and m['source_revision'] == SOURCE_REVISION
            and m['engine_version'] == ENGINE_VERSION, 'invalid Strata runtime identity')
    require(set(m) == {'schema', 'engine_version', 'source_revision', 'platform', 'architecture', 'backend', 'executable', 'python', 'files'},
            'unexpected Strata runtime metadata')
    require(m['platform'] in ('linux', 'windows', 'macos') and m['architecture'] in ('x86_64', 'aarch64')
            and m['backend'] in catalog()['entries'][0]['backends'][m['platform']]
            and (m['backend'] != 'metal' or m['architecture'] == 'aarch64'), 'unsupported runtime platform/backend')
    require(m['executable'] == ('strata-inkling.exe' if m['platform'] == 'windows' else 'strata-inkling')
            and m['python'] == ('python/python.exe' if m['platform'] == 'windows' else 'python/bin/python3'),
            'unexpected runtime entry point')
    for field, expected in [('platform', platform), ('architecture', architecture), ('backend', backend)]:
        if expected is not None:
            require(m[field] == expected, 'Strata runtime differs: ' + field)
    inventory(root, m['files'], ('chiral-runtime.json',))
    allowed = {m['executable'], 'server.py', 'common.py', 'catalog.json'} | {'upstream/' + p for p in UPSTREAM_FILES}
    require(all(name in allowed or name.startswith(('python/', 'lib/')) for name in m['files']),
            'unreviewed runtime support file')
    for name in ('executable', 'python'):
        require(m[name] in m['files'], 'runtime entry is not inventoried')
        spec = importlib.util.spec_from_file_location('legacy_runtime_check', ROOT / 'scripts/provision-runtime.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.binary_identity(root / m[name], m['platform'], m['architecture'], 'executable')
    require(read_json(root / 'catalog.json') == catalog(), 'packaged catalog differs')
    for name, checksum in UPSTREAM_FILES.items():
        require(m['files']['upstream/' + name]['sha256'] == checksum, 'unpinned upstream support file')
    for name in ('common.py', 'server.py'):
        require(m['files'][name]['sha256'] == digest(ROOT / 'scripts/strata' / name), 'adapter identity differs')
    return m


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest='command', required=True)
    sub.add_parser('list')
    model = sub.add_parser('model')
    model.add_argument('--catalog-id', required=True)
    model.add_argument('--output', required=True)
    model.add_argument('--accept-license', required=True)
    model.add_argument('--source-dir')
    runtime = sub.add_parser('runtime')
    for name in ('source-dir', 'engine-file', 'engine-sha256', 'python-dir', 'output'):
        runtime.add_argument('--' + name, required=True)
    runtime.add_argument('--engine-libraries')
    runtime.add_argument('--platform', choices=['linux', 'windows', 'macos'], required=True)
    runtime.add_argument('--architecture', choices=['x86_64', 'aarch64'], required=True)
    runtime.add_argument('--backend', choices=['cpu', 'cuda', 'metal'], required=True)
    check = sub.add_parser('check')
    check.add_argument('--model-dir', required=True)
    check.add_argument('--runtime-dir', required=True)
    a = ap.parse_args()
    if a.command == 'list':
        print(json.dumps(catalog(), indent=2))
    elif a.command == 'model':
        install_model(a.catalog_id, a.output, a.accept_license, a.source_dir)
    elif a.command == 'runtime':
        install_runtime(a)
    else:
        validate_artifact(Path(a.model_dir))
        validate_runtime(a.runtime_dir)
        print(json.dumps({'verified': True, 'engine': 'strata', 'qualified': False}))


if __name__ == '__main__':
    main()
