"""Shared, offline catalog and inventory contract for the Strata launch runtime."""
import hashlib
import json
from pathlib import Path
import ctypes
import re
import subprocess
import sys

SOURCE_REVISION = 'b28ca95e3e6847f2d5e317994f253b66716deeaa'
ENGINE_VERSION = '0.2.22'
RUNTIME_SCHEMA = 'chiral-strata-runtime-v1'
ARTIFACT_SCHEMA = 'chiral-strata-artifact-v1'


def host_available_mib():
    """Available host RAM, including CUDA deployments that keep experts on CPU."""
    if sys.platform.startswith('linux'):
        text = Path('/proc/meminfo').read_text()
        match = re.search(r'^MemAvailable:\s+(\d+) kB$', text, re.M)
        require(match is not None, 'host memory observation unavailable')
        return int(match[1]) // 1024
    if sys.platform == 'darwin':
        text = subprocess.check_output(['/usr/bin/vm_stat'], text=True, timeout=5)
        page = re.search(r'page size of (\d+) bytes', text)
        require(page is not None, 'host page size unavailable')
        total = 0
        for field in ('Pages free', 'Pages inactive', 'Pages speculative'):
            match = re.search(r'^' + field + r':\s+(\d+)\.', text, re.M)
            require(match is not None, 'host memory observation unavailable')
            total += int(match[1])
        return total * int(page[1]) // 1024**2
    if sys.platform == 'win32':
        class MemoryStatus(ctypes.Structure):
            _fields_ = [('length', ctypes.c_ulong), ('load', ctypes.c_ulong)] + [
                (n, ctypes.c_ulonglong) for n in ('total', 'available', 'page_total', 'page_available', 'virtual_total', 'virtual_available', 'extended')]
        status = MemoryStatus()
        status.length = ctypes.sizeof(status)
        require(ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)), 'host memory observation unavailable')
        return status.available // 1024**2
    raise ValueError('unsupported host memory observer')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def relative(name):
    p = Path(name)
    require(isinstance(name, str) and name and not p.is_absolute()
            and '\\' not in name and ':' not in name
            and all(x not in ('', '.', '..') for x in name.split('/')), 'unsafe inventory path')
    return p


def read_json(path, limit=4 * 1024 * 1024):
    p = Path(path)
    require(not p.is_symlink() and p.is_file() and p.stat().st_size <= limit, 'invalid bounded JSON file')
    return json.loads(p.read_bytes())


def catalog(path=None):
    if path is None:
        path = Path(__file__).parents[2] / 'dev/strata/catalog.json'
    value = read_json(path)
    require(value['schema'] == 'chiral-strata-catalog-v1' and value['source_revision'] == SOURCE_REVISION,
            'unsupported Strata catalog')
    return value


def entry(name, path=None):
    matches = [e for e in catalog(path)['entries'] if e['id'] == name]
    require(len(matches) == 1, 'model is outside the curated Strata catalog')
    return matches[0]


def model_spec(e):
    return {'id': e['repo_id'] + '/' + e['model_file'], 'revision': e['revision'],
            'tokenizer_revision': e['revision'],
            'chat_template_sha256': e['files']['chat_template.jinja']['sha256'],
            'quantization': e['quantization'], 'context_length': e['context_length'],
            'max_output_tokens': e['max_output_tokens'], 'capabilities': e['capabilities'], 'serving': None}


def inventory(root, files, metadata=()):
    root = Path(root)
    require(root.is_dir() and not root.is_symlink() and 0 < len(files) <= 8192, 'invalid inventory root/count')
    expected = set(files) | set(metadata)
    actual = set()
    for p in root.rglob('*'):
        require(not p.is_symlink(), 'inventory symlinks are forbidden')
        if p.is_dir():
            continue
        require(p.is_file(), 'inventory contains a non-file')
        actual.add(p.relative_to(root).as_posix())
    require(actual == expected, 'inventory contains missing or unexpected files')
    for name, item in files.items():
        p = root / relative(name)
        require(p.stat().st_size == item['size_bytes'] and digest(p) == item['sha256'],
                'inventory checksum mismatch: ' + name)


def validate_artifact(root, catalog_path=None):
    root = Path(root)
    manifest = read_json(root / 'chiral-artifact-sha256.json')
    e = entry(manifest['catalog_id'], catalog_path)
    require(manifest == artifact_manifest(e), 'artifact differs from curated entry')
    inventory(root, e['files'], ('chiral-artifact-sha256.json', 'chiral-model.json'))
    model = read_json(root / 'chiral-model.json')
    require(model == model_spec(e), 'artifact ModelSpec differs from catalog')
    return model, 'generation'


def artifact_manifest(e):
    return {'schema': ARTIFACT_SCHEMA, 'catalog_id': e['id'], 'files': e['files'],
            'model': {'repo_id': e['repo_id'], 'revision': e['revision']},
            'model_file': e['model_file'], 'license': e['license'],
            'context_length': e['context_length'], 'max_output_tokens': e['max_output_tokens'],
            'capabilities': e['capabilities'], 'quantization': e['quantization'], 'task': 'generation'}
