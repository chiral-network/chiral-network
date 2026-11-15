#!/usr/bin/env python3
"""Install a SHA256-pinned llama.cpp b12110 native archive without executing it.

Supply --archive (local pinned source build or downloaded package), or --url for
an official b12110 release asset, and its independently obtained --sha256. Native
execution never requires Python. The immutable installed inventory contains only
llama-server, its libraries, Metal resources and license/readme files. Known
upstream CLI tools are not installed. Only bounded in-package library aliases
are materialized; other links are rejected.
"""
import argparse
import importlib.util
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import struct
import tarfile
import tempfile
import urllib.parse
import urllib.request
import zipfile

_spec = importlib.util.spec_from_file_location('chiral_gguf_artifacts', Path(__file__).with_name('provision-model.py'))
_artifact = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_artifact)
require = _artifact.require
safe_path = _artifact.safe_path
sha256_file = _artifact.sha256_file
canonical_json = _artifact.canonical_json
load_json = _artifact.load_json
inventory_paths = _artifact.inventory_paths

INVENTORY = 'chiral-runtime.json'
SCHEMA = 'chiral-llama-runtime-v1'
ENGINE_VERSION = 'v0.9.0'
SOURCE_REVISION = 'cd959bae99ff4bed22bef2604f45241c09bbb8c8'
BINARY_RELEASE = 'b12110'
# Digests from https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b12110
# (target_commitish == SOURCE_REVISION). Never refresh these at install time.
OFFICIAL_ASSETS = {
    'llama-b12110-bin-macos-arm64.tar.gz': ('4f5b8de12e0b9516658ca6a5764c6ffb3d7c659c3dd538556b1413c7b4050662', 'macos', 'aarch64', 'metal'),
    'llama-b12110-bin-macos-x64.tar.gz': ('c8094268b6bf62f4998f0e45656fd9f5e2d4afb6b1cd0044d761db64be1dd47d', 'macos', 'x86_64', 'metal'),
    'llama-b12110-bin-ubuntu-arm64.tar.gz': ('4758b51335e37b4346251f2687d64841ff1c07ecb27fc46323945bb2ec9c44d2', 'linux', 'aarch64', 'cpu'),
    'llama-b12110-bin-ubuntu-x64.tar.gz': ('368227edcc229be5f9bed16a2223d79b5e41d0ac2e19bcf5825e82291c060e55', 'linux', 'x86_64', 'cpu'),
    'llama-b12110-bin-ubuntu-cuda-12.8-x64.tar.gz': ('4af1e1da23661db4a5b84b29aae16ebc053df689c5b8b1427c4670f2c6406d8c', 'linux', 'x86_64', 'cuda'),
    'llama-b12110-bin-ubuntu-cuda-13.4-arm64.tar.gz': ('77b300bfc55955e5b6ae82cdb44fe4af8306f82d06309364b1e4e4a5c33242d6', 'linux', 'aarch64', 'cuda'),
    'llama-b12110-bin-ubuntu-cuda-13.4-x64.tar.gz': ('2afd2ca540da119570b1dac36b5a2c862bb03a3b789c46661dfa55e600f5b19b', 'linux', 'x86_64', 'cuda'),
    'llama-b12110-bin-ubuntu-vulkan-arm64.tar.gz': ('1c68bf23d35230cc607e3d30f8d447d21fef500d9fa8d65e15520dfa3619ad73', 'linux', 'aarch64', 'vulkan'),
    'llama-b12110-bin-ubuntu-vulkan-x64.tar.gz': ('a5d741940956ec62e8783aee7e5a4c7c761af867d6f027c3277a51e1a3ac59b0', 'linux', 'x86_64', 'vulkan'),
    'llama-b12110-bin-win-cpu-arm64.zip': ('f8d20fb07d0b499988ced8963afcd5ded2d2cdd9983b757fbc7e51c4abd722e8', 'windows', 'aarch64', 'cpu'),
    'llama-b12110-bin-win-cpu-x64.zip': ('98901e327c252875accaf2d1bf364e10e868843b0b63468baad1aa945cdc5da2', 'windows', 'x86_64', 'cpu'),
    'llama-b12110-bin-win-cuda-12.4-x64.zip': ('1ce755bc3a061889234048f88e33328e17971eaad73c4666cfeb26026b317fee', 'windows', 'x86_64', 'cuda'),
    'llama-b12110-bin-win-cuda-13.4-arm64.zip': ('b981f7b4d73d61459e3a4f9785f15f573277587aaa98ffc4b03c055c4f64135f', 'windows', 'aarch64', 'cuda'),
    'llama-b12110-bin-win-cuda-13.4-x64.zip': ('dc910a969190b5d42e189afd73117f6bbdd8dd8ed6586342c5e0b82cab62c673', 'windows', 'x86_64', 'cuda'),
    'llama-b12110-bin-win-vulkan-arm64.zip': ('2be672cf538aabfc9fcb22c21648422b01d623673f15c2742bd43a9bd1a11ce2', 'windows', 'aarch64', 'vulkan'),
    'llama-b12110-bin-win-vulkan-x64.zip': ('9732a5e48887ca54d0bfe4af4324dc85fd56cd8cda19b52df3542fa6ebe6874f', 'windows', 'x86_64', 'vulkan'),
}
MAX_ARCHIVE_BYTES = 16 * 1024**3
MAX_UNPACKED_BYTES = 32 * 1024**3
# Official release utilities are not runtime dependencies and are never installed.
UPSTREAM_TOOLS = {'llama', 'ggml-metal-tuning', 'ggml-rpc-server', 'llama-connect', 'llama-results', 'llama-mtmd-debug',
    'llama-batched', 'llama-batched-bench', 'llama-bench', 'llama-cli',
    'llama-completion', 'llama-convert-llama2c-to-ggml', 'llama-cvector-generator',
    'llama-debug', 'llama-diffusion-cli', 'llama-embedding', 'llama-eval-callback',
    'llama-export-lora', 'llama-finetune', 'llama-fit-params', 'llama-gemma3-cli',
    'llama-gen-docs', 'llama-gguf', 'llama-gguf-hash', 'llama-gguf-split',
    'llama-idle', 'llama-imatrix', 'llama-llava-cli', 'llama-lookahead',
    'llama-lookup', 'llama-lookup-create', 'llama-lookup-merge', 'llama-lookup-stats',
    'llama-minicpmv-cli', 'llama-mtmd-cli', 'llama-parallel', 'llama-passkey',
    'llama-perplexity', 'llama-qwen2vl-cli', 'llama-quantize', 'llama-retrieval',
    'llama-save-load-state', 'llama-simple', 'llama-simple-chat', 'llama-speculative',
    'llama-speculative-simple', 'llama-tokenize', 'llama-tts', 'llama-vdot'}
LIBRARY_STEM = r'(?:lib)?(?:llama(?:-common|-server-impl)?|ggml(?:-[A-Za-z0-9_-]+)?|mtmd|cudart(?:64_[0-9]+)?|cublas(?:Lt)?(?:64_[0-9]+)?|nvrtc(?:-builtins)?(?:64_[0-9]+)?|nvJitLink(?:_[0-9]+)?|gomp|omp|iomp5|stdc\+\+|gcc_s|vulkan(?:-1)?|OpenCL|vcruntime[0-9_]*|msvcp[0-9_]*)'


def runtime_file_kind(name, platform):
    base = PurePosixPath(name).name
    if base == ('llama-server.exe' if platform == 'windows' else 'llama-server'):
        return 'executable'
    suffix = {'windows': r'\.dll', 'linux': r'\.so(?:\.[0-9]+)*', 'macos': r'(?:\.[0-9]+)*\.dylib'}[platform]
    if re.fullmatch(LIBRARY_STEM + suffix, base, re.IGNORECASE if platform == 'windows' else 0):
        return 'library'
    if platform == 'macos' and base in ('ggml-metal.metal', 'ggml-metal.metallib'):
        return 'resource'
    if re.fullmatch(r'(?:LICENSE|COPYING|NOTICE|README)(?:[._-][A-Za-z0-9._-]+)?', base, re.IGNORECASE):
        return 'documentation'
    return None


def binary_identity(path, platform, architecture, kind):
    """Check native format/machine without invoking a candidate executable."""
    with path.open('rb') as source:
        header = source.read(64)
        if platform == 'linux':
            require(len(header) == 64 and header[:4] == b'\x7fELF' and header[4] == 2 and header[5] == 1, 'runtime must be little-endian ELF64')
            require(struct.unpack_from('<H', header, 18)[0] == {'x86_64':62, 'aarch64':183}[architecture], 'runtime ELF architecture differs')
            require(struct.unpack_from('<H', header, 16)[0] in ((3,) if kind == 'library' else (2, 3)), 'runtime ELF is not the declared executable/shared library type')
        elif platform == 'windows':
            require(len(header) == 64 and header[:2] == b'MZ', 'runtime must be PE')
            offset = struct.unpack_from('<I', header, 60)[0]
            require(64 <= offset <= 1024 * 1024, 'invalid PE header offset')
            source.seek(offset)
            pe = source.read(26)
            require(len(pe) == 26 and pe[:4] == b'PE\0\0' and struct.unpack_from('<H', pe, 4)[0] == {'x86_64':0x8664, 'aarch64':0xaa64}[architecture] and struct.unpack_from('<H', pe, 24)[0] == 0x20b, 'runtime PE architecture/format differs')
            require(bool(struct.unpack_from('<H', pe, 22)[0] & 0x2000) == (kind == 'library'), 'runtime PE executable/library type differs')
        else:
            cpu = {'x86_64':0x01000007, 'aarch64':0x0100000c}[architecture]
            require(len(header) >= 32, 'truncated Mach-O')
            if header[:4] == b'\xcf\xfa\xed\xfe':
                require(struct.unpack_from('<I', header, 4)[0] == cpu and struct.unpack_from('<I', header, 12)[0] in ((6, 8) if kind == 'library' else (2,)), 'runtime Mach-O architecture/type differs')
            else:
                require(header[:4] in (b'\xca\xfe\xba\xbe', b'\xca\xfe\xba\xbf'), 'runtime must be Mach-O')
                count = struct.unpack_from('>I', header, 4)[0]
                require(0 < count <= 8, 'invalid universal Mach-O')
                source.seek(8)
                stride = 32 if header[:4] == b'\xca\xfe\xba\xbf' else 20
                slices = source.read(count * stride)
                require(len(slices) == count * stride, 'truncated universal Mach-O')
                selected = [i for i in range(count) if struct.unpack_from('>I', slices, i * stride)[0] == cpu]
                require(len(selected) == 1, 'runtime universal architecture differs')
                entry = selected[0] * stride
                offset, size = struct.unpack_from('>QQ' if stride == 32 else '>II', slices, entry + 8)
                require(offset >= 8 + len(slices) and size >= 32 and offset + size <= path.stat().st_size, 'invalid universal Mach-O slice')
                source.seek(offset)
                thin = source.read(32)
                require(thin[:4] == b'\xcf\xfa\xed\xfe' and struct.unpack_from('<I', thin, 4)[0] == cpu and
                        struct.unpack_from('<I', thin, 12)[0] in ((6, 8) if kind == 'library' else (2,)),
                        'universal Mach-O slice type differs')


def validate_runtime(root):
    """Return the verified manifest. Its canonical file SHA256 is runtime_sha256."""
    root = Path(root)
    actual_files = inventory_paths(root)
    manifest = load_json(root / INVENTORY)
    required = {'schema', 'platform', 'architecture', 'backend', 'engine_version', 'source_revision', 'executable', 'files'}
    require(isinstance(manifest, dict) and set(manifest) == required and manifest['schema'] == SCHEMA, 'unsupported runtime manifest')
    require((root / INVENTORY).read_bytes() == canonical_json(manifest), 'runtime manifest must use canonical JSON')
    require(manifest['engine_version'] == ENGINE_VERSION and manifest['source_revision'] == SOURCE_REVISION, 'runtime source/version is not pinned')
    platform, architecture, backend = manifest['platform'], manifest['architecture'], manifest['backend']
    require(platform in ('linux', 'windows', 'macos') and architecture in ('x86_64', 'aarch64'), 'unsupported runtime platform/architecture')
    require(backend in ('cpu', 'cuda', 'vulkan', 'metal') and (backend != 'metal' or platform == 'macos') and (backend != 'cuda' or platform != 'macos'), 'incompatible runtime backend')
    files = manifest['files']
    require(isinstance(files, dict) and 0 < len(files) <= 1024, 'invalid runtime inventory')
    require(actual_files == set(files) | {INVENTORY}, 'runtime has missing or unexpected files')
    executable = manifest['executable']
    safe_path(executable)
    require(executable in files and runtime_file_kind(executable, platform) == 'executable', 'invalid runtime executable')
    server_count = 0
    for name, entry in files.items():
        safe_path(name)
        kind = runtime_file_kind(name, platform)
        require(kind is not None, 'unexpected runtime file: ' + name)
        server_count += kind == 'executable'
        require(isinstance(entry, dict) and set(entry) == {'sha256', 'size_bytes'}, 'invalid runtime file entry')
        require(type(entry['size_bytes']) is int and entry['size_bytes'] > 0 and entry['size_bytes'] == (root / name).stat().st_size, 'runtime file size differs')
        require(isinstance(entry['sha256'], str) and re.fullmatch(r'[0-9a-f]{64}', entry['sha256']) and sha256_file(root / name) == entry['sha256'], 'runtime file hash differs')
        if kind in ('executable', 'library'):
            binary_identity(root / name, platform, architecture, kind)
        if kind == 'executable' and os.name != 'nt' and platform != 'windows':
            require(os.access(root / name, os.X_OK), 'runtime server is not executable')
    require(server_count == 1, 'exactly one runtime server required')
    return manifest


def _archive_role(name, platform):
    kind = runtime_file_kind(name, platform)
    if kind:
        return kind
    base = PurePosixPath(name).name
    tool = base[:-4] if base.endswith('.exe') else base
    if tool in UPSTREAM_TOOLS:
        return 'ignore'
    # Current upstream splits CLI tools and the server into implementation
    # libraries. Keep server/common libraries, but omit excluded tools' code.
    implementation = re.fullmatch(r'(?:lib)?(llama-[A-Za-z0-9_-]+)-impl(?:\.so(?:\.[0-9]+)*|(?:\.[0-9]+)*\.dylib|\.dll)', base)
    if implementation and implementation.group(1) in UPSTREAM_TOOLS:
        return 'ignore'
    require(False, 'unexpected archive file: ' + name)


def install_archive(archive, output, expected_sha256, platform, architecture, backend):
    """Install an explicitly digest-pinned ZIP/tar package; never uses extractall."""
    archive, output = Path(archive), Path(output)
    require(platform in ('linux', 'windows', 'macos') and architecture in ('x86_64', 'aarch64'), 'unsupported platform/architecture')
    require(isinstance(expected_sha256, str) and re.fullmatch(r'[0-9a-f]{64}', expected_sha256), 'explicit archive SHA256 required')
    require(archive.is_file() and not archive.is_symlink() and archive.stat().st_size <= MAX_ARCHIVE_BYTES and sha256_file(archive) == expected_sha256, 'runtime archive SHA256 differs')
    require(not output.exists() and not output.is_symlink(), 'runtime output must not exist')
    output.mkdir(parents=True, mode=0o755)
    files, names, total = {}, set(), 0
    try:
        def extract(name, size, mode, is_directory, source_factory):
            nonlocal total
            require(stat.S_IFMT(mode) in (0, stat.S_IFDIR if is_directory else stat.S_IFREG), 'archive links/special files forbidden')
            require(not mode & (stat.S_ISUID | stat.S_ISGID), 'privileged archive files forbidden')
            while name.startswith('./'):
                name = name[2:]
            if is_directory and name in ('', '.'):
                return
            if is_directory:
                name = name.rstrip('/')
            safe_path(name)
            require(name.casefold() not in names, 'duplicate/case-colliding archive member')
            names.add(name.casefold())
            require(len(names) <= 4096, 'too many archive members')
            if is_directory:
                return
            require(0 <= size <= MAX_UNPACKED_BYTES, 'excessive archive member size')
            total += size
            require(total <= MAX_UNPACKED_BYTES, 'archive expands beyond size limit')
            role = _archive_role(name, platform)
            if role == 'ignore':
                return
            target = output / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with source_factory() as source, target.open('xb') as dest:
                remaining = size
                while remaining:
                    chunk = source.read(min(remaining, 1024 * 1024))
                    require(chunk, 'truncated runtime archive member')
                    dest.write(chunk)
                    remaining -= len(chunk)
                require(not source.read(1), 'archive member exceeds declared size')
            target.chmod(0o755 if role in ('executable', 'library') and platform != 'windows' else 0o644)
            files[name] = {'sha256': sha256_file(target), 'size_bytes': size}
        if zipfile.is_zipfile(archive):
            with zipfile.ZipFile(archive) as package:
                for item in package.infolist():
                    require(not item.flag_bits & 1, 'encrypted archives forbidden')
                    extract(item.filename, item.file_size, item.external_attr >> 16, item.is_dir(), lambda item=item: package.open(item))
        else:
            with tarfile.open(archive, 'r:*') as package:
                members, folded = {}, set()
                for item in package:
                    name = item.name
                    while name.startswith('./'):
                        name = name[2:]
                    if item.isdir() and name in ('', '.'):
                        continue
                    if item.isdir():
                        name = name.rstrip('/')
                    safe_path(name)
                    require(name.casefold() not in folded, 'duplicate/case-colliding archive member')
                    folded.add(name.casefold())
                    require(len(folded) <= 4096, 'too many archive members')
                    members[name] = item
                for name, item in members.items():
                    source, source_name, visited = item, name, set()
                    while source.issym() or source.islnk():
                        require(runtime_file_kind(source_name, platform) == 'library',
                                'archive links must name packaged native libraries')
                        require(source_name not in visited and len(visited) < 32, 'cyclic or excessive archive links')
                        visited.add(source_name)
                        try:
                            target = safe_path(source.linkname)
                        except ValueError as error:
                            raise ValueError('archive links must stay inside the package') from error
                        source_name = str(PurePosixPath(source_name).parent / target) if source.issym() else str(target)
                        require(source_name in members and runtime_file_kind(source_name, platform) == 'library',
                                'archive links must resolve to packaged native libraries')
                        source = members[source_name]
                    require(source.isfile() or (source is item and source.isdir()), 'archive links/special files forbidden')
                    # Materialize verified in-package library aliases as regular
                    # bytes. No archive-controlled filesystem links are created.
                    extract(name, source.size, item.mode | (stat.S_IFDIR if source.isdir() else stat.S_IFREG),
                            source.isdir(), lambda source=source: package.extractfile(source))
        executables = [name for name in files if runtime_file_kind(name, platform) == 'executable']
        require(len(executables) == 1, 'archive must contain exactly one llama-server')
        manifest = {'schema': SCHEMA, 'platform': platform, 'architecture': architecture, 'backend': backend,
                    'engine_version': ENGINE_VERSION, 'source_revision': SOURCE_REVISION,
                    'executable': executables[0], 'files': files}
        (output / INVENTORY).write_bytes(canonical_json(manifest))
        validate_runtime(output)
        return manifest
    except BaseException:
        shutil.rmtree(output)
        raise


def check_official_asset(url_string, digest, platform, architecture, backend):
    url = urllib.parse.urlsplit(url_string)
    require(url.scheme == 'https' and url.netloc == 'github.com' and not url.query and not url.fragment and
            re.fullmatch(r'/ggml-org/llama\.cpp/releases/download/b12110/[A-Za-z0-9._+-]+', url.path),
            'URL must be an official pinned b12110 release asset')
    pin = OFFICIAL_ASSETS.get(PurePosixPath(url.path).name)
    require(pin is not None and pin == (digest, platform, architecture, backend),
            'release asset digest/platform/backend differs from compiled pin')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--archive', type=Path)
    source.add_argument('--url')
    parser.add_argument('--sha256', required=True, help='independently obtained immutable package SHA256')
    parser.add_argument('--source-revision', required=True, choices=[SOURCE_REVISION], help='attest local build/asset source pin')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--platform', choices=['linux', 'windows', 'macos'], required=True)
    parser.add_argument('--architecture', choices=['x86_64', 'aarch64'], required=True)
    parser.add_argument('--backend', choices=['cpu', 'cuda', 'metal', 'vulkan'], required=True)
    args = parser.parse_args()
    if args.url:
        check_official_asset(args.url, args.sha256, args.platform, args.architecture, args.backend)
        with tempfile.TemporaryDirectory(prefix='chiral-runtime-') as temp:
            archive = Path(temp) / 'runtime.archive'
            with urllib.request.urlopen(args.url, timeout=120) as response, archive.open('xb') as dest:
                require(urllib.parse.urlsplit(response.url).scheme == 'https', 'runtime download redirected away from HTTPS')
                total = 0
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    require(total <= MAX_ARCHIVE_BYTES, 'runtime archive too large')
                    dest.write(chunk)
            install_archive(archive, args.output, args.sha256, args.platform, args.architecture, args.backend)
    else:
        install_archive(args.archive, args.output, args.sha256, args.platform, args.architecture, args.backend)
    print('runtime_sha256=' + sha256_file(args.output / INVENTORY))


if __name__ == '__main__':
    main()
