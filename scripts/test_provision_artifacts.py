#!/usr/bin/env python3
"""Behavioral regressions for offline GGUF/runtime supply-chain validation."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import stat
import struct
import sys
import tarfile
import tempfile
import types
import unittest
from unittest.mock import patch
import zipfile


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


model = module('provision_model_test', 'provision-model.py')
runtime = module('provision_runtime_test', 'provision-runtime.py')


def gguf_string(value):
    raw = value.encode() if isinstance(value, str) else value
    return struct.pack('<Q', len(raw)) + raw


def gguf_bytes(extra=None, tensor='weight', template='{{ messages }}', split=None):
    metadata = {'general.architecture': (8, 'llama'), 'general.file_type': (4, 0),
        'llama.context_length': (4, 2048), 'tokenizer.ggml.model': (8, 'llama'),
        'tokenizer.ggml.tokens': (9, ['a', 'b'])}
    if template:
        metadata['tokenizer.chat_template'] = (8, template)
    if split:
        index, count, total = split
        metadata.update({'split.no': (2, index), 'split.count': (2, count), 'split.tensors.count': (5, total)})
    metadata.update(extra or {})
    data = b'GGUF' + struct.pack('<IQQ', 3, 1, len(metadata))
    for key, (kind, value) in metadata.items():
        data += gguf_string(key) + struct.pack('<I', kind)
        if kind == 8:
            data += gguf_string(value)
        elif kind == 9:
            data += struct.pack('<IQ', 8, len(value)) + b''.join(gguf_string(item) for item in value)
        else:
            data += struct.pack('<' + model.GGUFReader.FORMATS[kind], value)
    data += gguf_string(tensor) + struct.pack('<IQIQ', 1, 1, 0, 0)
    return data + b'\0' * (-len(data) % 32) + struct.pack('<f', 1.0)


def make_artifact(root, shards=None, task='generation', extra=None, template='{{ messages }}'):
    root.mkdir()
    shards = shards or {'model-F32.gguf': gguf_bytes(extra=extra, template=template)}
    for name, data in shards.items():
        target = root / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    first, quantization, selected = model.inspect_gguf_set(root, list(shards), task, None, 512)
    manifest = {'schema': model.SCHEMA, 'model': {'repo_id': 'owner/model-GGUF', 'revision': 'a' * 40, 'license': 'mit'},
        'task': task, 'quantization': quantization, 'context_length': 512,
        'max_output_tokens': 0 if task == 'embeddings' else 32,
        'capabilities': ['embeddings'] if task == 'embeddings' else (['chat', 'completions'] if selected else ['completions']),
        'chat_template_name': None, 'model_file': first, 'files': {}}
    for name in shards:
        manifest['files'][name] = {'sha256': model.sha256_file(root / name), 'size_bytes': (root / name).stat().st_size,
            'upstream_repo': manifest['model']['repo_id'], 'upstream_path': name, 'upstream_digest': model.sha256_file(root / name)}
    if selected:
        target = root / 'chiral-chat-template.jinja'
        target.write_bytes(selected.encode())
        manifest['files'][target.name] = {'sha256': model.sha256_file(target), 'size_bytes': target.stat().st_size,
            'upstream_repo': None, 'upstream_path': None, 'upstream_digest': None}
    (root / model.INVENTORY).write_bytes(model.canonical_json(manifest))
    (root / 'chiral-model.json').write_bytes(model.canonical_json(model.model_spec(manifest, selected)))
    return manifest


def elf(machine=62):
    header = bytearray(64)
    header[:6] = b'\x7fELF\x02\x01'
    struct.pack_into('<HH', header, 16, 3, machine)
    return bytes(header)


def pe(machine=0x8664):
    header = bytearray(64)
    header[:2] = b'MZ'
    struct.pack_into('<I', header, 60, 64)
    body = bytearray(26)
    body[:4] = b'PE\0\0'
    struct.pack_into('<H', body, 4, machine)
    struct.pack_into('<H', body, 24, 0x20b)
    return bytes(header + body)


def macho(cpu=0x0100000c):
    return b'\xcf\xfa\xed\xfe' + struct.pack('<IIIII', cpu, 0, 2, 0, 0) + b'\0' * 8


class ArtifactValidation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'artifact'

    def test_generation_identity_and_embedded_template(self):
        make_artifact(self.root)
        base, task = model.validate_artifact(self.root)
        self.assertEqual(task, 'generation')
        self.assertEqual(base['id'], 'owner/model-GGUF/model-F32.gguf')
        self.assertEqual(base['revision'], base['tokenizer_revision'])
        self.assertEqual(base['capabilities'], ['chat', 'completions'])
        self.assertEqual(base['quantization'], 'F32')

    def test_template_identity_uses_effective_source_without_trimming_content(self):
        make_artifact(self.root, template=' \r\n{{ messages }}\r\n\r\n')
        base, _ = model.validate_artifact(self.root)
        expected = hashlib.sha256(b' \n{{ messages }}\n').hexdigest()
        self.assertEqual(base['chat_template_sha256'], expected)
        manifest = model.load_json(self.root / model.INVENTORY)
        self.assertNotEqual(manifest['files']['chiral-chat-template.jinja']['sha256'], expected)

    def test_complete_split_artifact(self):
        shards = {f'model-0000{i + 1}-of-00002.gguf': gguf_bytes(tensor=f'weight{i}', split=(i, 2, 2)) for i in range(2)}
        make_artifact(self.root, shards=shards)
        self.assertEqual(model.validate_artifact(self.root)[1], 'generation')
        (self.root / 'model-00002-of-00002.gguf').unlink()
        with self.assertRaisesRegex(ValueError, 'missing or unexpected'):
            model.validate_artifact(self.root)

    def test_missing_shard_and_wrong_split_indices_rejected(self):
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            model.shard_names(['model-00001-of-00002.gguf'])
        self.root.mkdir()
        for i in range(2):
            (self.root / f'model-{i + 1:05d}-of-00002.gguf').write_bytes(gguf_bytes(tensor=str(i), split=(0, 2, 2)))
        with self.assertRaisesRegex(ValueError, 'split metadata'):
            model.inspect_gguf_set(self.root, [p.name for p in self.root.iterdir()], 'generation', None, 512)

    def test_embeddings_do_not_publish_chat_even_with_embedded_template(self):
        make_artifact(self.root, task='embeddings', extra={'llama.pooling_type': (4, 1), 'llama.attention.causal': (7, 0)})
        base, task = model.validate_artifact(self.root)
        self.assertEqual((task, base['capabilities'], base['max_output_tokens']), ('embeddings', ['embeddings'], 0))
        self.assertFalse((self.root / 'chiral-chat-template.jinja').exists())

    def test_generation_rejects_embedding_model(self):
        with self.assertRaisesRegex(ValueError, 'cannot serve generation'):
            make_artifact(self.root, extra={'llama.pooling_type': (4, 1)})

    def test_embedding_requires_vector_pooling(self):
        with self.assertRaisesRegex(ValueError, 'vector pooling'):
            make_artifact(self.root, task='embeddings')

    def test_corruption_unexpected_file_and_template_changes_rejected(self):
        make_artifact(self.root)
        file = self.root / 'model-F32.gguf'
        original = file.read_bytes()
        file.write_bytes(original[:-1] + b'x')
        with self.assertRaisesRegex(ValueError, 'digest differs'):
            model.validate_artifact(self.root)
        file.write_bytes(original)
        (self.root / 'run.py').write_text('raise RuntimeError')
        with self.assertRaisesRegex(ValueError, 'unexpected'):
            model.validate_artifact(self.root)
        (self.root / 'run.py').unlink()
        (self.root / 'chiral-chat-template.jinja').write_text('bad template')
        with self.assertRaises(ValueError):
            model.validate_artifact(self.root)

    def test_template_cannot_be_replaced_even_with_inventory_rehashed(self):
        manifest = make_artifact(self.root)
        target = self.root / 'chiral-chat-template.jinja'
        target.write_text('{{ different }}')
        manifest['files'][target.name].update(sha256=model.sha256_file(target), size_bytes=target.stat().st_size)
        (self.root / model.INVENTORY).write_bytes(model.canonical_json(manifest))
        with self.assertRaisesRegex(ValueError, 'template differs'):
            model.validate_artifact(self.root)

    def test_named_template_is_explicit_and_bound(self):
        self.root.mkdir()
        (self.root / 'model.gguf').write_bytes(gguf_bytes(template='', extra={'tokenizer.chat_template.tool_use': (8, '{{ tools }}')}))
        with self.assertRaisesRegex(ValueError, 'explicit named'):
            model.inspect_gguf_set(self.root, ['model.gguf'], 'generation', None, 512)
        self.assertEqual(model.inspect_gguf_set(self.root, ['model.gguf'], 'generation', 'tool_use', 512)[2], '{{ tools }}')
        with self.assertRaisesRegex(ValueError, 'cannot select'):
            model.inspect_gguf_set(self.root, ['model.gguf'], 'embeddings', 'tool_use', 512)

    def test_truncated_tensor_and_excessive_metadata_rejected(self):
        self.root.mkdir()
        path = self.root / 'bad.gguf'
        path.write_bytes(gguf_bytes()[:-1])
        with self.assertRaisesRegex(ValueError, 'truncated'):
            model.read_gguf(path)
        path.write_bytes(b'GGUF' + struct.pack('<IQQ', 3, 1, 2**63))
        with self.assertRaisesRegex(ValueError, 'counts'):
            model.read_gguf(path)

    def test_traversal_and_windows_aliases_rejected(self):
        for name in ('../x.gguf', '/x.gguf', 'C:/x.gguf', 'a\\x.gguf', './x.gguf', 'a//x.gguf', 'NUL.gguf', 'a./x.gguf'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                model.safe_path(name)

    def test_symlinks_rejected(self):
        make_artifact(self.root)
        link = self.root / 'extra.gguf'
        try:
            link.symlink_to('model-F32.gguf')
        except OSError:
            self.skipTest('symlinks unavailable to current account')
        with self.assertRaisesRegex(ValueError, 'links'):
            model.validate_artifact(self.root)

    def test_noncanonical_or_duplicate_manifest_rejected(self):
        manifest = make_artifact(self.root)
        (self.root / model.INVENTORY).write_text(json.dumps(manifest, indent=2))
        with self.assertRaisesRegex(ValueError, 'canonical'):
            model.validate_artifact(self.root)
        (self.root / model.INVENTORY).write_text('{"schema":1,"schema":2}')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            model.validate_artifact(self.root)


class ProvisioningCapabilities(unittest.TestCase):
    """public-inference-api requirement 7 (task 3.7): `provision-model.py` itself, run end to
    end against a stand-in for `huggingface_hub` that serves the GGUF bytes of one pinned
    commit, derives `completions` for generation, adds `chat` only when an embedded template
    is selected, derives exactly `embeddings` for the embedding task, and adds no `tools`,
    `structured_output` or `streaming` because of a model card, a repository or file name or
    embedded metadata. No network, download or Hub package is used."""
    REVISION = 'c' * 40

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)

    def provision(self, files, output, task='generation', template_name=None,
                  repo='owner/model-GGUF', card=None):
        """Runs `main()` for `files` (selected name -> GGUF bytes) of `repo` at REVISION and
        returns the verified base ModelSpec, the task and the inventory manifest."""
        cache = self.base / ('hub-cache-' + output)
        siblings = []
        for name, data in files.items():
            target = cache / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            siblings.append(types.SimpleNamespace(rfilename=name, blob_id=None,
                lfs=types.SimpleNamespace(sha256=hashlib.sha256(data).hexdigest())))
        info = types.SimpleNamespace(sha=self.REVISION, siblings=siblings,
                                     card_data=dict(card or {}, license='mit'))
        hub = types.ModuleType('huggingface_hub')
        hub.HfApi = lambda: types.SimpleNamespace(
            model_info=lambda repo_id, revision, files_metadata: info)
        hub.hf_hub_download = lambda repo_id, filename, revision: str(cache / filename)
        root = self.base / output
        argv = ['provision-model.py', '--output', str(root), '--repo', repo,
                '--revision', self.REVISION, '--task', task, '--context-length', '512',
                '--accept-license', 'mit']
        for name in files:
            argv += ['--gguf-file', name]
        if template_name:
            argv += ['--chat-template-name', template_name]
        with patch.dict(sys.modules, {'huggingface_hub': hub}), patch.object(sys, 'argv', argv), \
                contextlib.redirect_stdout(io.StringIO()):
            model.main()
        base, verified_task = model.validate_artifact(root)
        return base, verified_task, model.load_json(root / model.INVENTORY)

    def test_generation_derives_chat_only_from_a_selected_embedded_template(self):
        template = '{{ messages }}\n'
        base, task, manifest = self.provision({'with-template.gguf': gguf_bytes(template=template)}, 'templated')
        self.assertEqual((task, base['capabilities'], manifest['capabilities']),
                         ('generation', ['chat', 'completions'], ['chat', 'completions']))
        self.assertEqual((self.base / 'templated' / 'chiral-chat-template.jinja').read_bytes(), template.encode())
        self.assertEqual(base['chat_template_sha256'], hashlib.sha256(b'{{ messages }}').hexdigest())
        # The artifact identity: the full revision, the GGUF inventory, the quantization and
        # the template hash; the tokenizer revision is the GGUF revision.
        self.assertEqual((base['revision'], base['tokenizer_revision'], base['quantization']),
                         (self.REVISION, self.REVISION, 'F32'))
        self.assertEqual(set(manifest['files']), {'with-template.gguf', 'chiral-chat-template.jinja'})

        base, task, manifest = self.provision({'without-template.gguf': gguf_bytes(template='')}, 'untemplated')
        self.assertEqual((task, base['capabilities'], manifest['capabilities']),
                         ('generation', ['completions'], ['completions']))
        self.assertFalse((self.base / 'untemplated' / 'chiral-chat-template.jinja').exists())
        self.assertEqual(set(manifest['files']), {'without-template.gguf'})
        self.assertEqual(manifest['chat_template_name'], None)

    def test_an_embedding_gguf_with_an_embedded_template_derives_only_embeddings(self):
        embedding = gguf_bytes(extra={'llama.pooling_type': (4, 1), 'llama.attention.causal': (7, 0)},
                               template='{{ messages }}')
        base, task, manifest = self.provision({'embed.gguf': embedding}, 'embedding', task='embeddings')
        self.assertEqual((task, base['capabilities'], base['max_output_tokens']), ('embeddings', ['embeddings'], 0))
        self.assertEqual(base['chat_template_sha256'], hashlib.sha256(b'').hexdigest())
        self.assertEqual(set(manifest['files']), {'embed.gguf'})
        self.assertFalse((self.base / 'embedding' / 'chiral-chat-template.jinja').exists())
        # Selecting the embedded template for the embedding task is refused, and nothing stays.
        with self.assertRaisesRegex(ValueError, 'cannot select'):
            self.provision({'embed.gguf': embedding}, 'embedding-with-template', task='embeddings',
                           template_name='default')
        self.assertFalse((self.base / 'embedding-with-template').exists())

    def test_a_tool_use_template_and_model_card_claims_add_no_capability(self):
        claims = ['function-calling', 'tool-use', 'tools', 'json-mode', 'structured-output', 'streaming']
        gguf = gguf_bytes(template='', extra={
            'tokenizer.chat_template.tool_use': (8, '{% for tool in tools %}{{ tool }}{% endfor %}{{ messages }}'),
            'general.name': (8, 'Tools JSON-Schema Streaming Function-Calling Instruct'),
            'general.tags': (9, claims)})
        files = {'tools-json-schema-streaming-function-calling-F32.gguf': gguf}
        repo = 'owner/Tools-JSON-Streaming-Function-Calling-GGUF'
        card = {'tags': claims, 'pipeline_tag': 'text-generation', 'library_name': 'transformers'}
        base, task, manifest = self.provision(files, 'tool-use', template_name='tool_use', repo=repo, card=card)
        self.assertEqual((task, base['capabilities'], manifest['capabilities']),
                         ('generation', ['chat', 'completions'], ['chat', 'completions']))
        for claimed in ('tools', 'structured_output', 'streaming'):
            self.assertNotIn(claimed, base['capabilities'])
        self.assertEqual(manifest['chat_template_name'], 'tool_use')
        self.assertEqual((self.base / 'tool-use' / 'chiral-chat-template.jinja').read_text(),
                         '{% for tool in tools %}{{ tool }}{% endfor %}{{ messages }}')
        # Only named templates: one must be selected explicitly; nothing is chosen for it.
        with self.assertRaisesRegex(ValueError, 'explicit named'):
            self.provision(files, 'tool-use-unselected', repo=repo, card=card)
        self.assertFalse((self.base / 'tool-use-unselected').exists())

    def test_only_gguf_files_prepare_or_execute_a_model(self):
        # No separate tokenizer, safetensors or code path is selectable or accepted.
        for name in ('model.safetensors', 'tokenizer.json', 'tokenizer.model', 'modeling.py'):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'only GGUF'):
                model.shard_names([name])
        root = self.base / 'artifact'
        manifest = make_artifact(root)
        (root / 'tokenizer.json').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'missing or unexpected'):
            model.validate_artifact(root)
        manifest['files']['tokenizer.json'] = {'sha256': model.sha256_file(root / 'tokenizer.json'),
            'size_bytes': (root / 'tokenizer.json').stat().st_size, 'upstream_repo': None,
            'upstream_path': None, 'upstream_digest': None}
        (root / model.INVENTORY).write_bytes(model.canonical_json(manifest))
        with self.assertRaisesRegex(ValueError, 'unexpected artifact file'):
            model.validate_artifact(root)


class RuntimeValidation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def package(self, entries):
        path = self.root / 'runtime.zip'
        with zipfile.ZipFile(path, 'w') as archive:
            for name, content in entries.items():
                archive.writestr(name, content)
        return path

    def install(self, archive, platform='linux', architecture='x86_64', backend='cpu'):
        return runtime.install_archive(archive, self.root / 'installed', model.sha256_file(archive), platform, architecture, backend)

    def test_linux_windows_macos_inventory(self):
        for platform, architecture, name, data in [('linux', 'x86_64', 'llama-server', elf()),
                ('windows', 'x86_64', 'llama-server.exe', pe()), ('macos', 'aarch64', 'llama-server', macho())]:
            with self.subTest(platform=platform):
                archive = self.package({'bin/' + name: data})
                installed = self.install(archive, platform, architecture)
                self.assertEqual(installed['source_revision'], runtime.SOURCE_REVISION)
                self.assertEqual(runtime.validate_runtime(self.root / 'installed'), installed)
                shutil.rmtree(self.root / 'installed')

    def test_macos_package_omits_offline_tuner_but_rejects_unknown_tools(self):
        entries = {'bin/llama-server': macho(), 'bin/ggml-metal-tuning': macho()}
        manifest = self.install(self.package(entries), 'macos', 'aarch64', 'metal')
        self.assertEqual((self.root / 'installed/bin/llama-server').read_bytes(), macho())
        self.assertNotIn('bin/ggml-metal-tuning', manifest['files'])
        self.assertFalse((self.root / 'installed/bin/ggml-metal-tuning').exists())
        shutil.rmtree(self.root / 'installed')
        entries['bin/ggml-metal-unreviewed'] = macho()
        with self.assertRaisesRegex(ValueError, 'unexpected archive file'):
            self.install(self.package(entries), 'macos', 'aarch64', 'metal')

    def test_hash_mismatch_fails_before_output_creation(self):
        archive = self.package({'llama-server': elf()})
        with self.assertRaisesRegex(ValueError, 'SHA256'):
            runtime.install_archive(archive, self.root / 'installed', '0' * 64, 'linux', 'x86_64', 'cpu')
        self.assertFalse((self.root / 'installed').exists())

    def test_shared_library_tampering_and_unlisted_library_rejected(self):
        self.install(self.package({'llama-server': elf(), 'libllama.so': elf()}))
        root = self.root / 'installed'
        (root / 'libllama.so').write_bytes(elf() + b'x')
        with self.assertRaisesRegex(ValueError, 'size differs'):
            runtime.validate_runtime(root)
        (root / 'libllama.so').write_bytes(elf())
        (root / 'libggml-cuda.so').write_bytes(elf())
        with self.assertRaisesRegex(ValueError, 'unexpected'):
            runtime.validate_runtime(root)

    def test_traversal_unexpected_executable_case_collision_and_wrong_machine(self):
        for entries in ({'../llama-server': elf()}, {'llama-server': elf(), 'evil.exe': pe()},
                        {'llama-server': elf(), 'LLAMA-SERVER': elf()}, {'llama-server': elf(183)}):
            with self.subTest(entries=list(entries)), self.assertRaises(ValueError):
                self.install(self.package(entries))
            self.assertFalse((self.root / 'installed').exists())

    def test_zip_symlink_rejected(self):
        path = self.root / 'link.zip'
        with zipfile.ZipFile(path, 'w') as archive:
            info = zipfile.ZipInfo('llama-server')
            info.create_system = 3
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(info, '/usr/bin/sh')
        with self.assertRaisesRegex(ValueError, 'links'):
            self.install(path)

    def test_tar_hardlink_rejected(self):
        path = self.root / 'link.tar'
        with tarfile.open(path, 'w') as archive:
            info = tarfile.TarInfo('llama-server')
            info.type, info.linkname = tarfile.LNKTYPE, '/bin/sh'
            archive.addfile(info)
        with self.assertRaisesRegex(ValueError, 'links'):
            self.install(path)

    def test_tar_library_aliases_resolve_without_creating_filesystem_links(self):
        path = self.root / 'libraries.tar'
        with tarfile.open(path, 'w') as archive:
            for name in ['bin/llama-server', 'bin/libllama.so.1']:
                info = tarfile.TarInfo(name)
                info.size, info.mode = len(elf()), 0o755
                archive.addfile(info, io.BytesIO(elf()))
            info = tarfile.TarInfo('bin/libllama.so')
            info.type, info.linkname = tarfile.SYMTYPE, 'libllama.so.1'
            archive.addfile(info)
        self.install(path)
        alias = self.root / 'installed' / 'bin' / 'libllama.so'
        self.assertFalse(alias.is_symlink())
        self.assertEqual(alias.read_bytes(), elf())

    def test_tar_library_link_cycle_rejected(self):
        path = self.root / 'cycle.tar'
        with tarfile.open(path, 'w') as archive:
            for name, target in [('libllama.so', 'libllama.so.1'), ('libllama.so.1', 'libllama.so')]:
                info = tarfile.TarInfo(name)
                info.type, info.linkname = tarfile.SYMTYPE, target
                archive.addfile(info)
        with self.assertRaisesRegex(ValueError, 'cyclic'):
            self.install(path)

    def test_tar_regular_package_installs(self):
        path = self.root / 'runtime.tar.gz'
        with tarfile.open(path, 'w:gz') as archive:
            info = tarfile.TarInfo('bin/llama-server')
            info.size, info.mode = len(elf()), 0o755
            archive.addfile(info, io.BytesIO(elf()))
        self.assertEqual(self.install(path)['executable'], 'bin/llama-server')

    def test_other_pinned_release_rejected(self):
        self.install(self.package({'llama-server': elf()}))
        path = self.root / 'installed' / runtime.INVENTORY
        manifest = model.load_json(path)
        manifest['engine_version'] = 'latest'
        path.write_bytes(model.canonical_json(manifest))
        with self.assertRaisesRegex(ValueError, 'not pinned'):
            runtime.validate_runtime(path.parent)

    def test_official_download_cannot_change_digest_or_platform(self):
        name = 'llama-b12110-bin-win-vulkan-x64.zip'
        url = 'https://github.com/ggml-org/llama.cpp/releases/download/b12110/' + name
        digest, platform, architecture, backend = runtime.OFFICIAL_ASSETS[name]
        runtime.check_official_asset(url, digest, platform, architecture, backend)
        for candidate, candidate_digest, candidate_platform in (
                (url, '0' * 64, platform), (url, digest, 'linux'),
                (url.replace('b12110/', 'latest/'), digest, platform),
                (url.replace('github.com', 'example.com'), digest, platform)):
            with self.subTest(url=candidate), self.assertRaises(ValueError):
                runtime.check_official_asset(candidate, candidate_digest, candidate_platform, architecture, backend)

    def test_split_server_package_accepts_required_libraries_but_rejects_unknown_code(self):
        entries = {'llama-server': elf(), 'libllama-common.so': elf(),
                   'libllama-server-impl.so': elf(), 'libllama-perplexity-impl.so': elf()}
        manifest = self.install(self.package(entries))
        self.assertIn('libllama-server-impl.so', manifest['files'])
        self.assertNotIn('libllama-perplexity-impl.so', manifest['files'])
        shutil.rmtree(self.root / 'installed')
        entries['libunknown-impl.so'] = elf()
        with self.assertRaisesRegex(ValueError, 'unexpected archive file'):
            self.install(self.package(entries))

    def test_incompatible_backend_rejected(self):
        with self.assertRaisesRegex(ValueError, 'backend'):
            self.install(self.package({'llama-server': elf()}), backend='metal')

    def test_disguised_windows_executable_is_not_a_library(self):
        with self.assertRaisesRegex(ValueError, 'type differs'):
            self.install(self.package({'llama-server.exe': pe(), 'ggml.dll': pe()}), platform='windows')


if __name__ == '__main__':
    unittest.main()
