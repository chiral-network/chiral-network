#!/usr/bin/env python3
"""Provision explicit GGUF files from a full Hugging Face commit, without model code.

Download requires huggingface_hub; validation uses only the Python standard library.
Select every shard with repeated --gguf-file. Acknowledging --accept-license records
acceptance of the repository's declared license; it is not legal advice. Runtime
capability and performance qualification remain separate from artifact validation.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re
import shutil
import struct

INVENTORY = 'chiral-artifact-sha256.json'
SCHEMA = 'chiral-gguf-artifact-v1'
MAX_METADATA_BYTES = 2 * 1024 * 1024
# llama.cpp cd959bae99ff4bed22bef2604f45241c09bbb8c8: llama_ftype.
FILE_TYPES = dict(enumerate(('F32', 'F16', 'Q4_0', 'Q4_1', None, None, None,
    'Q8_0', 'Q5_0', 'Q5_1', 'Q2_K', 'Q3_K_S', 'Q3_K_M', 'Q3_K_L', 'Q4_K_S',
    'Q4_K_M', 'Q5_K_S', 'Q5_K_M', 'Q6_K', 'IQ2_XXS', 'IQ2_XS', 'Q2_K_S',
    'IQ3_XS', 'IQ3_XXS', 'IQ1_S', 'IQ4_NL', 'IQ3_S', 'IQ3_M', 'IQ2_S',
    'IQ2_M', 'IQ4_XS', 'IQ1_M', 'BF16', None, None, None, 'TQ1_0', 'TQ2_0',
    'MXFP4_MOE', 'NVFP4', 'Q1_0', 'Q2_0', 'F8_E4M3')))
# Tensor block elements and bytes, from pinned gguf/constants.py.
TENSOR_TYPES = {0:(1,4), 1:(1,2), 2:(32,18), 3:(32,20), 6:(32,22), 7:(32,24),
    8:(32,34), 9:(32,36), 10:(256,84), 11:(256,110), 12:(256,144),
    13:(256,176), 14:(256,210), 15:(256,292), 16:(256,66), 17:(256,74),
    18:(256,98), 19:(256,50), 20:(32,18), 21:(256,110), 22:(256,82),
    23:(256,136), 24:(1,1), 25:(1,2), 26:(1,4), 27:(1,8), 28:(1,8),
    29:(256,56), 30:(1,2), 34:(256,54), 35:(256,66), 39:(32,17),
    40:(64,36), 41:(128,18), 42:(64,18), 43:(1,1)}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical_json(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode('utf-8')


def sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def load_json(path):
    require(path.is_file() and not path.is_symlink(), 'metadata must be a regular file')
    require(path.stat().st_size <= MAX_METADATA_BYTES, 'metadata exceeds size limit')
    return json.loads(path.read_text(encoding='utf-8'), object_pairs_hook=_unique_object)


def safe_path(name):
    require(isinstance(name, str) and 0 < len(name) <= 512, 'invalid artifact path')
    path = PurePosixPath(name)
    require(not path.is_absolute() and str(path) == name and
            all(part not in ('', '.', '..') and not part.endswith((' ', '.')) and
                not re.fullmatch(r'(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?', part)
                for part in path.parts) and
            not any(ord(c) < 32 or c in '\\:<>"|?*' for c in name), 'unsafe artifact path')
    return path


def check_identity(identity):
    require(isinstance(identity, dict) and set(identity) == {'repo_id', 'revision', 'license'}, 'invalid HF provenance')
    require(isinstance(identity['repo_id'], str) and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*', identity['repo_id']), 'HF repository must be organization/name')
    require(isinstance(identity['revision'], str) and re.fullmatch(r'[0-9a-f]{40}', identity['revision']), 'HF revision must be an immutable full commit')
    require(isinstance(identity['license'], str) and 0 < len(identity['license']) <= 128, 'explicit upstream license identifier required')


def upstream_digest(path, expected):
    if len(expected) == 64:
        return sha256_file(path)
    digest = hashlib.sha1()
    digest.update(f'blob {path.stat().st_size}\0'.encode())
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def inventory_paths(root):
    """Reject links, special files and case-colliding names on every platform."""
    require(root.is_dir() and not root.is_symlink(), 'root must be a real directory')
    files, seen = set(), set()
    for path in root.rglob('*'):
        name = path.relative_to(root).as_posix()
        safe_path(name)
        require(name.casefold() not in seen, 'case-colliding inventory paths')
        seen.add(name.casefold())
        require(not path.is_symlink() and not (hasattr(path, 'is_junction') and path.is_junction()), 'links are forbidden')
        if not path.is_dir():
            require(path.is_file(), 'only regular files are permitted')
            files.add(name)
    return files


class GGUFReader:
    """Bounded metadata reader; never maps tensors or materializes tokenizer arrays."""
    FORMATS = {0:'B', 1:'b', 2:'H', 3:'h', 4:'I', 5:'i', 6:'f', 7:'B', 10:'Q', 11:'q', 12:'d'}

    def __init__(self, source, size):
        self.source, self.size = source, size
        self.endian = '<'
        self.metadata_limit = min(size, 512 * 1024 * 1024)

    def take(self, length):
        require(0 <= length <= self.metadata_limit - self.source.tell(), 'truncated or excessive GGUF metadata')
        data = self.source.read(length)
        require(len(data) == length, 'truncated GGUF')
        return data

    def number(self, fmt):
        return struct.unpack(self.endian + fmt, self.take(struct.calcsize(fmt)))[0]

    def string(self, decode=True, limit=MAX_METADATA_BYTES):
        length = self.number('Q')
        require(length <= limit, 'GGUF string exceeds size limit')
        raw = self.take(length)
        return raw.decode('utf-8') if decode else raw

    def value(self, kind):
        if kind == 8:
            return self.string()
        if kind == 9:
            item_type, count = self.number('I'), self.number('Q')
            require(item_type != 9 and item_type in (*self.FORMATS, 8) and count <= 2_000_000, 'invalid GGUF array')
            digest = hashlib.sha256()
            if item_type == 8:
                for _ in range(count):
                    raw = self.string(decode=False)
                    digest.update(struct.pack('<Q', len(raw)))
                    digest.update(raw)
            else:
                width = struct.calcsize(self.FORMATS[item_type])
                remaining = count * width
                while remaining:
                    raw = self.take(min(remaining, 1024 * 1024))
                    digest.update(raw)
                    remaining -= len(raw)
            return {'type': item_type, 'count': count, 'sha256': digest.hexdigest()}
        require(kind in self.FORMATS, 'unknown GGUF metadata type')
        value = self.number(self.FORMATS[kind])
        require(kind != 7 or value in (0, 1), 'invalid GGUF boolean')
        require(not isinstance(value, float) or math.isfinite(value), 'nonfinite GGUF metadata')
        return bool(value) if kind == 7 else value


def read_gguf(path):
    with Path(path).open('rb') as source:
        reader = GGUFReader(source, Path(path).stat().st_size)
        require(reader.take(4) == b'GGUF', 'not a GGUF model')
        version = reader.number('I')
        if version in (0x02000000, 0x03000000):
            reader.endian = '>'
            version >>= 24
        require(version in (2, 3), 'unsupported GGUF version')
        tensor_count, kv_count = reader.number('Q'), reader.number('Q')
        require(0 <= tensor_count <= 1_000_000 and 0 < kv_count <= 100_000, 'excessive GGUF counts')
        metadata = {}
        for _ in range(kv_count):
            key = reader.string(limit=1024)
            require(key and key not in metadata, 'duplicate or empty GGUF metadata key')
            metadata[key] = reader.value(reader.number('I'))
        tensors, intervals = set(), []
        for _ in range(tensor_count):
            name = reader.string(limit=1024)
            require(name and name not in tensors, 'duplicate or empty tensor name')
            tensors.add(name)
            dimensions = reader.number('I')
            require(1 <= dimensions <= 4, 'invalid tensor rank')
            shape = [reader.number('Q') for _ in range(dimensions)]
            require(all(0 < n <= 2**40 for n in shape), 'invalid tensor dimensions')
            kind, offset = reader.number('I'), reader.number('Q')
            require(kind in TENSOR_TYPES, 'unsupported tensor encoding for pinned runtime')
            block, width = TENSOR_TYPES[kind]
            elements = math.prod(shape)
            require(shape[0] % block == 0, 'invalid quantized tensor shape')
            intervals.append((offset, offset + elements // block * width))
        alignment = metadata.get('general.alignment', 32)
        require(type(alignment) is int and 1 <= alignment <= 4096 and alignment & (alignment - 1) == 0, 'invalid GGUF alignment')
        start = (source.tell() + alignment - 1) // alignment * alignment
        previous_end = 0
        for begin, end in sorted(intervals):
            require(begin % alignment == 0 and begin >= previous_end and start + end <= reader.size, 'overlapping, truncated or unaligned GGUF tensor')
            previous_end = end
        return metadata, tensors


def shard_names(names):
    require(isinstance(names, (list, tuple)) and 0 < len(names) <= 1024, 'select a bounded GGUF shard set')
    require(len(names) == len(set(names)), 'duplicate selected GGUF')
    for name in names:
        safe_path(name)
        require(name.endswith('.gguf'), 'only GGUF model files are permitted')
    ordered = sorted(names)
    match = re.fullmatch(r'(.+)-(\d{5})-of-(\d{5})\.gguf', ordered[0])
    if match:
        prefix, _, count = match.groups()
        require(0 < int(count) <= 1024, 'invalid shard count')
        require(ordered == [f'{prefix}-{i:05d}-of-{count}.gguf' for i in range(1, int(count) + 1)], 'incomplete or mixed GGUF shards')
    else:
        require(len(ordered) == 1, 'multiple unsharded GGUF files selected')
    return ordered


def inspect_gguf_set(root, names, task, selected, context):
    ordered = shard_names(names)
    first, all_tensors = None, set()
    total = None
    for index, name in enumerate(ordered):
        metadata, tensors = read_gguf(root / name)
        require(not all_tensors.intersection(tensors), 'duplicate tensors across GGUF shards')
        all_tensors.update(tensors)
        if first is None:
            first = metadata
            total = metadata.get('split.tensors.count')
        if len(ordered) > 1:
            require(metadata.get('split.no') == index and metadata.get('split.count') == len(ordered) and metadata.get('split.tensors.count') == total, 'GGUF split metadata differs')
        else:
            require(metadata.get('split.count', 1) == 1 and metadata.get('split.no', 0) == 0, 'GGUF requires additional shards')
        for key in first.keys() & metadata.keys():
            if not key.startswith('split.'):
                require(first[key] == metadata[key], 'inconsistent GGUF shard metadata: ' + key)
    require(all_tensors and (total is None or total == len(all_tensors)), 'incomplete GGUF tensor set')
    require(first.get('general.type', 'model') == 'model', 'adapters are not model artifacts')
    architecture = first.get('general.architecture')
    require(isinstance(architecture, str) and re.fullmatch(r'[a-z0-9_-]+', architecture), 'missing GGUF model architecture')
    native_context = first.get(architecture + '.context_length')
    require(type(native_context) is int and 128 <= context <= native_context, 'context exceeds GGUF model context')
    tokenizer = first.get('tokenizer.ggml.model')
    tokens = first.get('tokenizer.ggml.tokens')
    require(isinstance(tokenizer, str) and tokenizer not in ('', 'none') and isinstance(tokens, dict) and tokens.get('type') == 8 and tokens.get('count', 0) > 0, 'embedded GGUF tokenizer required')
    quantization = FILE_TYPES.get(first.get('general.file_type'))
    require(quantization is not None, 'explicit supported GGUF file type required')
    causal = first.get(architecture + '.attention.causal', True)
    pooling = first.get(architecture + '.pooling_type', 0)
    require(task in ('generation', 'embeddings'), 'unsupported task')
    if task == 'embeddings':
        require(selected is None, 'embedding task cannot select a chat template')
        require(type(pooling) is int and pooling in (1, 2, 3), 'embedding model requires vector pooling metadata (not reranking)')
        template = ''
    else:
        require(causal is True and pooling == 0, 'embedding/reranker GGUF cannot serve generation')
        require(selected is None or isinstance(selected, str) and re.fullmatch(r'[A-Za-z0-9_-]{1,64}', selected), 'invalid template name')
        key = 'tokenizer.chat_template' if selected in (None, 'default') else 'tokenizer.chat_template.' + selected
        template = first.get(key, '')
        require(isinstance(template, str), 'invalid embedded chat template')
        require(selected is None or template, 'selected chat template is absent')
        require(template or not any(key.startswith('tokenizer.chat_template.') for key in first), 'select an explicit named chat template')
    return ordered[0], quantization, template


def template_source(template):
    # Pinned llama.cpp Jinja lexer normalizes line endings and removes exactly
    # one trailing LF, not arbitrary whitespace. Inventory retains original bytes.
    normalized = template.replace('\r\n', '\n').replace('\r', '\n')
    return normalized[:-1] if template.endswith('\n') else normalized


def model_spec(manifest, template):
    context, output, task = manifest['context_length'], manifest['max_output_tokens'], manifest['task']
    require(type(context) is int and 128 <= context <= 32768, 'context must be 128..32768')
    require(type(output) is int and (output == 0 if task == 'embeddings' else 0 < output < context), 'output limit is incompatible with task/context')
    capabilities = ['embeddings'] if task == 'embeddings' else (['chat', 'completions'] if template else ['completions'])
    require(manifest['capabilities'] == capabilities, 'capabilities differ from task/template')
    return {'id': manifest['model']['repo_id'] + '/' + PurePosixPath(manifest['model_file']).name,
            'revision': manifest['model']['revision'], 'tokenizer_revision': manifest['model']['revision'],
            'chat_template_sha256': hashlib.sha256(template_source(template).encode('utf-8')).hexdigest(),
            'quantization': manifest['quantization'], 'context_length': context,
            'max_output_tokens': output, 'capabilities': capabilities, 'serving': None}


def validate_artifact(root):
    """Verify complete bytes, embedded metadata and provenance; return (base ModelSpec, task)."""
    root = Path(root)
    actual_files = inventory_paths(root)
    manifest = load_json(root / INVENTORY)
    required = {'schema', 'model', 'task', 'quantization', 'context_length', 'max_output_tokens',
                'capabilities', 'chat_template_name', 'model_file', 'files'}
    require(isinstance(manifest, dict) and set(manifest) == required and manifest['schema'] == SCHEMA, 'unsupported artifact manifest')
    require((root / INVENTORY).read_bytes() == canonical_json(manifest), 'manifest must use canonical JSON')
    check_identity(manifest['model'])
    files = manifest['files']
    require(isinstance(files, dict) and 0 < len(files) <= 1025, 'invalid artifact inventory')
    require(actual_files == set(files) | {INVENTORY, 'chiral-model.json'}, 'artifact has missing or unexpected files')
    for name, entry in files.items():
        safe_path(name)
        require(name.endswith('.gguf') or name == 'chiral-chat-template.jinja', 'unexpected artifact file')
        require(isinstance(entry, dict) and set(entry) == {'sha256', 'size_bytes', 'upstream_repo', 'upstream_path', 'upstream_digest'}, 'invalid inventory entry')
        path = root / name
        require(type(entry['size_bytes']) is int and entry['size_bytes'] == path.stat().st_size, 'artifact size differs: ' + name)
        require(isinstance(entry['sha256'], str) and re.fullmatch(r'[0-9a-f]{64}', entry['sha256']), 'invalid SHA256')
        actual = sha256_file(path)
        require(actual == entry['sha256'], 'artifact digest differs: ' + name)
        if name == 'chiral-chat-template.jinja':
            require(all(entry[k] is None for k in ('upstream_repo', 'upstream_path', 'upstream_digest')), 'template must derive from GGUF')
        else:
            source = entry['upstream_digest']
            require(isinstance(source, str) and re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', source), 'invalid upstream digest')
            require(entry['upstream_repo'] == manifest['model']['repo_id'] and entry['upstream_path'] == name, 'artifact provenance differs')
            require((actual if len(source) == 64 else upstream_digest(path, source)) == source, 'artifact differs from pinned upstream')
    require(type(manifest['context_length']) is int, 'invalid context length')
    model_file, quantization, template = inspect_gguf_set(root, [n for n in files if n.endswith('.gguf')], manifest['task'], manifest['chat_template_name'], manifest['context_length'])
    require(manifest['model_file'] == model_file and manifest['quantization'] == quantization, 'model file or quantization differs from GGUF')
    require(('chiral-chat-template.jinja' in files) == bool(template), 'template inventory differs')
    if template:
        require((root / 'chiral-chat-template.jinja').read_bytes() == template.encode('utf-8'), 'template differs from GGUF')
    expected = model_spec(manifest, template)
    require(load_json(root / 'chiral-model.json') == expected, 'ModelSpec differs from verified artifact')
    return expected, manifest['task']


def artifact_manifest(root):
    validate_artifact(root)
    return load_json(Path(root) / INVENTORY)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repo', required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--gguf-file', action='append', required=True)
    parser.add_argument('--task', choices=['generation', 'embeddings'], required=True)
    parser.add_argument('--context-length', type=int, required=True)
    parser.add_argument('--max-output-tokens', type=int)
    parser.add_argument('--chat-template-name')
    parser.add_argument('--accept-license', action='append', required=True)
    args = parser.parse_args()
    require(not args.output.exists() and not args.output.is_symlink(), 'output must not exist')
    names = shard_names(args.gguf_file)
    identity = {'repo_id': args.repo, 'revision': args.revision, 'license': 'pending'}
    check_identity(identity)
    from huggingface_hub import HfApi, hf_hub_download
    info = HfApi().model_info(repo_id=args.repo, revision=args.revision, files_metadata=True)
    require(info.sha == args.revision, 'Hub resolved a different commit')
    license_id = (info.card_data or {}).get('license')
    require(isinstance(license_id, str) and license_id in args.accept_license, 'explicit acknowledgement required for upstream license: ' + str(license_id))
    identity['license'] = license_id
    siblings = {item.rfilename: item for item in info.siblings}
    require(all(name in siblings for name in names), 'selected GGUF absent from pinned repository')
    manifest = {'schema': SCHEMA, 'model': identity, 'task': args.task, 'quantization': None,
                'context_length': args.context_length,
                'max_output_tokens': args.max_output_tokens if args.max_output_tokens is not None else (0 if args.task == 'embeddings' else 256),
                'capabilities': [], 'chat_template_name': args.chat_template_name, 'model_file': names[0], 'files': {}}
    args.output.mkdir(parents=True, mode=0o755)
    try:
        for name in names:
            item = siblings[name]
            digest = item.lfs.sha256 if item.lfs else item.blob_id
            require(isinstance(digest, str) and re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', digest), 'Hub omitted immutable file digest')
            cached = Path(hf_hub_download(repo_id=args.repo, filename=name, revision=args.revision))
            require(upstream_digest(cached, digest) == digest, 'download differs from upstream digest')
            target = args.output / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(cached, target)
            manifest['files'][name] = {'sha256': sha256_file(target), 'size_bytes': target.stat().st_size,
                'upstream_repo': args.repo, 'upstream_path': name, 'upstream_digest': digest}
        first, quantization, template = inspect_gguf_set(args.output, names, args.task, args.chat_template_name, args.context_length)
        manifest.update(model_file=first, quantization=quantization,
                        capabilities=['embeddings'] if args.task == 'embeddings' else (['chat', 'completions'] if template else ['completions']))
        if template:
            target = args.output / 'chiral-chat-template.jinja'
            target.write_bytes(template.encode('utf-8'))
            manifest['files'][target.name] = {'sha256': sha256_file(target), 'size_bytes': target.stat().st_size,
                'upstream_repo': None, 'upstream_path': None, 'upstream_digest': None}
        (args.output / INVENTORY).write_bytes(canonical_json(manifest))
        (args.output / 'chiral-model.json').write_bytes(canonical_json(model_spec(manifest, template)))
        validate_artifact(args.output)
    except BaseException:
        shutil.rmtree(args.output)
        raise
    print('Pinned GGUF artifact verified. Real runtime capability/performance qualification remains required.')
    print('artifact_manifest_sha256=' + sha256_file(args.output / INVENTORY))


if __name__ == '__main__':
    main()
