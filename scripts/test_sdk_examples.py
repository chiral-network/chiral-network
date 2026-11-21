"""inference-request-accounting, requirement *Request identity prevents automatic paid replay*
(task 2.3): the supplied Python SDK example and the Usage page's Python SDK snippet send one
paid request and never retry it automatically, with a new key or at another provider.

Static checks of the parsed source with `ast`: the `openai` package is not installed here and
nothing is sent. `tests/sdkExamples.test.ts` checks the JavaScript example and snippet. The
checks do not show how the SDK itself behaves at run time."""
import ast
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parent.parent
EXAMPLE = ROOT / 'examples' / 'openai_client.py'
USAGE = ROOT / 'src' / 'pages' / 'Usage.svelte'
TRANSPORTS = {'requests', 'httpx', 'urllib', 'urllib3', 'aiohttp', 'http', 'socket', 'tenacity', 'backoff', 'retrying',
              'importlib', 'runpy', 'multiprocessing'}
# Calls that start or replace a process (`subprocess`, `os`, `pty`).
PROCESSES = {'run', 'call', 'check_call', 'check_output', 'Popen', 'getoutput', 'getstatusoutput', 'system', 'popen',
             'fork', 'forkpty', 'spawn', 'posix_spawn', 'posix_spawnp', 'startfile', 'execl', 'execle', 'execlp', 'execlpe',
             'execv', 'execve', 'execvp', 'execvpe', 'spawnl', 'spawnle', 'spawnlp', 'spawnlpe', 'spawnv', 'spawnve',
             'spawnvp', 'spawnvpe'}
LOOPS = (ast.For, ast.AsyncFor, ast.While, ast.ListComp, ast.SetComp, ast.DictComp, ast.GeneratorExp, ast.Lambda)
FUNCTIONS = (ast.FunctionDef, ast.AsyncFunctionDef)


def usage_snippet(name):
    """The uninterpolated template literal `const <name> = `...`;` in Usage.svelte's script."""
    source = USAGE.read_text(encoding='utf-8')
    marker = f'const {name} = `'
    if source.count(marker) != 1:
        raise AssertionError(f'expected one {name} snippet in {USAGE.name}')
    start = source.index(marker) + len(marker)
    end = source.index('`', start)
    text = source[start:end]
    # Without interpolation or escapes the literal's value is exactly its source text.
    if '${' in text or '\\' in text or source[end:end + 2] != '`;':
        raise AssertionError(f'{name} is not a plain template literal')
    return text


def dotted(node):
    """`a.b.c` for a name/attribute chain, otherwise None."""
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        base = dotted(node.value)
        return f'{base}.{node.attr}' if base else None
    return None


def callee(call):
    function = call.func
    if isinstance(function, ast.Attribute):
        return function.attr
    if isinstance(function, ast.Name):
        return function.id
    return None


def keywords(call):
    return {keyword.arg: keyword.value for keyword in call.keywords} if call else {}


def strings(root):
    return [node.value for node in ast.walk(root) if isinstance(node, ast.Constant) and isinstance(node.value, str)]


def audit(source, kind):
    """Every way `source` departs from a single, never-retried paid request; empty when it has
    none. `example` is the standalone script, which must also generate, print and send one
    Idempotency-Key, take its base URL from the explicit credential export and, on failure, return
    a non-zero status naming the key to inspect. `snippet` is the Usage page's environment-based
    snippet, which has no key and no except clause, so a failure propagates."""
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return ['syntax: the source does not parse']
    findings = []

    def find(message):
        if message not in findings:
            findings.append(message)

    parent = {child: node for node in ast.walk(tree) for child in ast.iter_child_nodes(node)}
    calls = [node for node in ast.walk(tree) if isinstance(node, ast.Call)]

    def path(node):
        """`node` and its ancestors, innermost first."""
        chain = [node]
        while chain[-1] in parent:
            chain.append(parent[chain[-1]])
        return chain

    # Only the SDK sends: no other HTTP client or retry helper.
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name.split('.')[0] in TRANSPORTS:
                    find(f'transport: imports {alias.name}')
        elif isinstance(node, ast.ImportFrom) and (node.module or '').split('.')[0] in TRANSPORTS:
            find(f'transport: imports {node.module}')

    # One client, with SDK retries disabled.
    clients = [call for call in calls if dotted(call.func) == 'OpenAI']
    if len(clients) != 1:
        find(f'retries: {len(clients)} OpenAI clients, expected one')
    options = keywords(clients[0] if clients else None)
    retries = options.get('max_retries')
    if not (isinstance(retries, ast.Constant) and type(retries.value) is int and retries.value == 0):
        find('retries: the client does not set max_retries=0')
    if any(keyword.arg == 'max_retries' and keyword.value is not retries for node in calls for keyword in node.keywords):
        find('retries: another max_retries setting')
    if any(callee(call) in ('with_options', 'copy') for call in calls):
        find('retries: a client copy with other options')
    if any(keyword.arg is None for node in calls if dotted(node.func) == 'OpenAI' or callee(node) == 'create' for keyword in node.keywords):
        find('retries: keyword unpacking can carry other client or request options')

    # Nothing changes the client or the exported credentials after construction: the SDK
    # client's `max_retries` and `base_url` attributes can be reassigned, and a value written
    # into the credentials would replace the exported base URL.
    for node in ast.walk(tree):
        written = None
        if isinstance(node, (ast.Attribute, ast.Subscript)) and isinstance(node.ctx, (ast.Store, ast.Del)):
            written = node.attr if isinstance(node, ast.Attribute) else ast.unparse(node)
        elif isinstance(node, ast.Call) and callee(node) in ('setattr', 'delattr'):
            written = ' '.join(strings(node)) or ast.unparse(node)
        elif isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and dotted(node.func.value) == 'credentials' \
                and node.func.attr != 'clear':
            written = f'credentials.{node.func.attr}'
        if written is not None:
            find('retries: a retry setting is changed after construction' if 'retr' in written
                 else 'base-url: the client or the credentials are changed after construction')

    # No process other than the explicit credential export, and no dynamic code, can run the
    # request again (for example by re-running the script after a failure).
    processes = [call for call in calls if callee(call) in PROCESSES]
    exports = [call for call in processes if dotted(call.func) == 'subprocess.run' and call.args
               and {'credentials', 'export'} <= set(strings(call.args[0]))]
    if len(processes) != (1 if kind == 'example' else 0) or len(exports) != len(processes):
        find('retry: a process other than the credential export is started')
    if any(callee(call) in ('__import__', 'import_module', 'exec', 'eval', 'compile', 'run_path', 'run_module') for call in calls):
        find('transport: dynamic import or code execution')

    # Exactly one paid call, made once: not in a loop, an except handler or a finally block.
    creates = [call for call in calls if callee(call) == 'create']
    if len(creates) != 1:
        find(f'one-call: {len(creates)} create calls, expected one')
    create = creates[0] if creates else None
    if create is not None and dotted(create.func) != 'client.chat.completions.create':
        find('one-call: the call is not client.chat.completions.create')
    chain = path(create) if create is not None else []
    for child, ancestor in zip(chain, chain[1:]):
        if isinstance(ancestor, LOOPS):
            find('loop: the create call is inside a loop')
        if isinstance(ancestor, ast.ExceptHandler):
            find('retry: the create call is inside an except handler')
        if isinstance(ancestor, ast.Try) and (child in ancestor.finalbody or child in ancestor.orelse):
            find('retry: the create call is inside a finally or else block')
    functions = [node for node in chain if isinstance(node, FUNCTIONS)]
    if kind == 'snippet':
        if functions:
            find('loop: the create call is inside a function that can run again')
    else:
        # The call is inside `main`, a top-level function run once by the `__main__` guard.
        runs = [call for call in calls if dotted(call.func) == 'main']
        guard = [node for node in tree.body if isinstance(node, ast.If) and ast.unparse(node.test) == "__name__ == '__main__'"]
        once = (
            [node.name for node in functions] == ['main'] and parent[functions[0]] is tree
            and len(runs) == 1 and len(guard) == 1 and runs[0] in set(ast.walk(guard[0]))
            and not any(isinstance(node, LOOPS + FUNCTIONS) for node in path(runs[0]))
        )
        if not once:
            find('loop: the create call is inside a function that can run again')

    # The base URL (and, for the snippet, key and model) come only from the exported credential.
    base = options.get('base_url')
    if any('://' in text for text in strings(tree)):
        find('base-url: a literal URL can name another provider')
    if kind == 'snippet':
        environment = {
            'base_url': (base, 'CHIRAL_BASE_URL'),
            'api_key': (options.get('api_key'), 'CHIRAL_API_KEY'),
            'model': (keywords(create).get('model'), 'CHIRAL_MODEL'),
        }
        for member, (value, variable) in environment.items():
            if value is None or ast.unparse(value) != f"os.environ['{variable}']":
                find(f'base-url: {member} is not exactly os.environ["{variable}"]')
    else:
        if base is None or ast.unparse(base) != "credentials['base_url']":
            find("base-url: the base URL is not the exported credential's base_url")
        stores = [node for node in ast.walk(tree) if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Store)]
        assigned = {name: [node for node in stores if node.id == name] for name in ('credentials', 'result')}
        exported = parent.get(assigned['credentials'][0]) if len(assigned['credentials']) == 1 else None
        command = parent.get(assigned['result'][0]) if len(assigned['result']) == 1 else None
        argv = command.value.args[0] if isinstance(command, ast.Assign) and isinstance(command.value, ast.Call) \
            and dotted(command.value.func) == 'subprocess.run' and command.value.args else None
        words = strings(argv) if isinstance(argv, ast.List) else []
        if not (isinstance(exported, ast.Assign) and ast.unparse(exported.value) == 'json.loads(result.stdout)'
                and 'credentials' in words and 'export' in words):
            find('base-url: credentials are not assigned once from the explicit credential export')

    if kind == 'example':
        # One key, generated once before the call, printed for reconciliation and sent with the call.
        generated = [call for call in calls if callee(call) in ('uuid1', 'uuid3', 'uuid4', 'uuid5', 'uuid7', 'token_hex', 'token_urlsafe')]
        if len(generated) != 1:
            find(f'key: {len(generated)} generated keys, expected one')
        statement = parent.get(parent.get(generated[0])) if generated else None
        key = statement.targets[0].id if isinstance(statement, ast.Assign) and len(statement.targets) == 1 \
            and isinstance(statement.targets[0], ast.Name) else None
        stores = [node for node in ast.walk(tree) if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Store) and node.id == key]
        once = (
            key is not None and len(stores) == 1 and create is not None and statement.lineno < create.lineno
            and ast.unparse(statement.value).startswith('str(')
            and all(isinstance(node, (ast.Module, ast.FunctionDef)) for node in path(statement)[1:])
        )
        if not once:
            find('key: the key is not one name assigned once, outside any loop or block, before the call')
        headers = keywords(create).get('extra_headers')
        sent = isinstance(headers, ast.Dict) and len(headers.keys) == 1 and isinstance(headers.keys[0], ast.Constant) \
            and headers.keys[0].value == 'Idempotency-Key' and isinstance(headers.values[0], ast.Name) and headers.values[0].id == key
        if not sent:
            find('key: the call does not send the generated key as its Idempotency-Key')
        if sum(text.lower() == 'idempotency-key' for text in strings(tree)) != 1 or 'default_headers' in options:
            find('key: another Idempotency-Key')

        def mentions(root):
            """f-strings under `root` that format the key."""
            return [node for node in ast.walk(root) if isinstance(node, ast.JoinedStr)
                    and any(isinstance(value, ast.FormattedValue) and ast.unparse(value.value) == key for value in node.values)]
        if key is None or create is None or not any(node.lineno < create.lineno and any('Request key' in text for text in strings(node)) for node in mentions(tree)):
            find('key: the key is not printed before the call')

        # On failure: no second call, a non-zero status and the key to inspect.
        tries = [ancestor for child, ancestor in zip(chain, chain[1:]) if isinstance(ancestor, ast.Try) and child in ancestor.body]
        if not tries or not tries[0].handlers:
            find('failure: the call is not inside a try block with an except handler')
        else:
            handlers = tries[0].handlers
            caught = {dotted(node) for handler in handlers if handler.type is not None
                      for node in (handler.type.elts if isinstance(handler.type, ast.Tuple) else [handler.type])}
            if not (caught & {'Exception', 'BaseException'} or any(handler.type is None for handler in handlers)):
                find('failure: a failure of the call is not handled')
            for handler in handlers:
                last = handler.body[-1]
                status = isinstance(last, ast.Return) and isinstance(last.value, ast.Constant) \
                    and type(last.value.value) is int and last.value.value != 0
                if not (status or isinstance(last, ast.Raise)):
                    find('failure: the except handler does not end with a non-zero status')
                if not any('Do not automatically replay' in text for text in strings(handler)):
                    find('failure: the except handler does not warn against replay')
                if not any(any('Inspect with' in text for text in strings(node)) and any(' request ' in text for text in strings(node))
                           for node in mentions(handler)):
                    find('failure: the except handler does not name the request key to inspect')
            # The status reaches the process exit code.
            if not any(ast.unparse(call) == 'sys.exit(main())' for call in calls):
                find('failure: the returned status is not the exit status')
    else:
        # The snippet catches nothing, so a failure propagates; its finally only closes the stream.
        if any(isinstance(node, ast.ExceptHandler) for node in ast.walk(tree)):
            find('failure: an except handler can swallow a failure')
        closes = any(isinstance(node, ast.Try) and any(ast.unparse(statement) == 'stream.close()' for statement in node.finalbody)
                     for node in ast.walk(tree))
        if not closes:
            find('failure: no finally block closes the stream')
    return findings


def mutate(source, *edits):
    """`source` with the single occurrence of each `old` replaced by its `new`."""
    for old, new in edits:
        if source.count(old) != 1:
            raise AssertionError(f'mutation anchor {old!r} occurs {source.count(old)} times')
        source = source.replace(old, new)
    return source


class SuppliedPythonClientsNeverRetryPaidInference(unittest.TestCase):
    example = EXAMPLE.read_text(encoding='utf-8')
    snippet = usage_snippet('python')

    def test_example_sends_one_keyed_request_from_the_export_and_fails_with_the_key_to_inspect(self):
        self.assertEqual(audit(self.example, 'example'), [])

    def test_usage_snippet_sends_one_request_from_the_environment_and_lets_a_failure_propagate(self):
        self.assertEqual(audit(self.snippet, 'snippet'), [])

    def check(self, source, kind, finding, *edits):
        findings = audit(mutate(source, *edits), kind)
        self.assertNotIn('syntax: the source does not parse', findings)
        self.assertIn(finding, findings)

    def test_each_example_check_fails_on_a_mutated_copy(self):
        create = '            with client.chat.completions.create('
        handler = '        print("\\nGeneration interrupted. Do not automatically replay this paid request.", file=sys.stderr)\n'
        inspect = '        print(f"Inspect with: chiral --config <config> request {args.contract} {request_key}", file=sys.stderr)\n'
        cases = [
            ('SDK retries enabled', 'retries: the client does not set max_retries=0', ('max_retries=0', 'max_retries=2')),
            ('a per-request retry setting', 'retries: another max_retries setting',
             ('                max_tokens=128,', '                max_tokens=128, max_retries=2,')),
            ('a client copy', 'retries: a client copy with other options',
             (create, '            client = client.with_options(max_retries=2)\n' + create)),
            ('a retrying HTTP client', 'transport: imports httpx', ('import uuid\n', 'import uuid\nimport httpx\n')),
            ('a loop around the call', 'loop: the create call is inside a loop',
             (create, '            for attempt in range(2):\n             with client.chat.completions.create(')),
            ('a second call in the except handler', 'one-call: 2 create calls, expected one',
             (handler, handler + '        client.chat.completions.create(model=credentials["model"], messages=[], extra_headers={"Idempotency-Key": str(uuid.uuid4())})\n')),
            ('rerunning main from the except handler', 'loop: the create call is inside a function that can run again',
             (handler, handler + '        return main()\n')),
            ('a new key per attempt', 'key: 2 generated keys, expected one',
             ('extra_headers={"Idempotency-Key": request_key}', 'extra_headers={"Idempotency-Key": str(uuid.uuid4())}')),
            ('a key that is not sent', 'key: the call does not send the generated key as its Idempotency-Key',
             ('extra_headers={"Idempotency-Key": request_key}', 'extra_headers={"Idempotency-Key": "fixed"}')),
            ('a key generated inside the try block', 'key: the key is not one name assigned once, outside any loop or block, before the call',
             ('    request_key = str(uuid.uuid4())\n', ''), ('    try:\n        with OpenAI', '    try:\n        request_key = str(uuid.uuid4())\n        with OpenAI')),
            ('a key that is not printed', 'key: the key is not printed before the call',
             ('    print(f"Request key: {request_key}", file=sys.stderr)\n', '')),
            ('a fallback provider', 'base-url: a literal URL can name another provider',
             ('base_url=credentials["base_url"]', 'base_url=credentials.get("base_url") or "https://fallback.example/v1"')),
            ('a base URL from the environment', "base-url: the base URL is not the exported credential's base_url",
             ('base_url=credentials["base_url"]', 'base_url=args.prompt')),
            ('a zero status on failure', 'failure: the except handler does not end with a non-zero status',
             ('        return 1\n    finally:', '        return 0\n    finally:')),
            ('an unhandled SDK error', 'failure: a failure of the call is not handled',
             ('except (KeyboardInterrupt, Exception):', 'except KeyboardInterrupt:')),
            ('no key to inspect', 'failure: the except handler does not name the request key to inspect', (inspect, '')),
            ('no replay warning', 'failure: the except handler does not warn against replay', (handler, '')),
            ('a discarded status', 'failure: the returned status is not the exit status', ('sys.exit(main())', 'main()')),
            # The SDKs retry twice by default, so an omitted setting must fail too.
            ('no retry setting (the SDK default retries)', 'retries: the client does not set max_retries=0', (', max_retries=0', '')),
            ('retries enabled on the client afterwards', 'retries: a retry setting is changed after construction',
             (create, '            client.max_retries = 2\n' + create)),
            ('retries enabled through setattr', 'retries: a retry setting is changed after construction',
             (create, '            setattr(client, "max_retries", 2)\n' + create)),
            ('unpacked client options', 'retries: keyword unpacking can carry other client or request options',
             ('max_retries=0, timeout=120.0)', 'max_retries=0, timeout=120.0, **credentials.get("options", {}))')),
            ('another base URL set on the client', 'base-url: the client or the credentials are changed after construction',
             (create, '            client.base_url = args.prompt\n' + create)),
            ('a fallback written into the credentials', 'base-url: the client or the credentials are changed after construction',
             ('    request_key = str(uuid.uuid4())\n', '    credentials["base_url"] = credentials["base_url"] or args.prompt\n    request_key = str(uuid.uuid4())\n')),
            ('credentials updated with another base URL', 'base-url: the client or the credentials are changed after construction',
             ('    request_key = str(uuid.uuid4())\n', '    credentials.update(base_url=args.prompt)\n    request_key = str(uuid.uuid4())\n')),
            ('the script rerun from the except handler', 'retry: a process other than the credential export is started',
             (handler, handler + '        subprocess.run([sys.executable, *sys.argv], check=False)\n')),
            ('the process replaced from the except handler', 'retry: a process other than the credential export is started',
             ('import uuid\n', 'import uuid\nimport os\n'), (handler, handler + '        os.execv(sys.executable, [sys.executable, *sys.argv])\n')),
        ]
        for label, finding, *edits in cases:
            with self.subTest(label):
                self.check(self.example, 'example', finding, *edits)

    def test_each_snippet_check_fails_on_a_mutated_copy(self):
        cases = [
            ('SDK retries enabled', 'retries: the client does not set max_retries=0', ('max_retries=0,', 'max_retries=3,')),
            ('a loop around the call', 'loop: the create call is inside a loop',
             ('stream = client.chat.completions.create(', 'for attempt in range(2):\n stream = client.chat.completions.create(')),
            ('a second call after a caught failure', 'one-call: 2 create calls, expected one',
             ('finally:', 'except Exception:\n    client.chat.completions.create(model=os.environ["CHIRAL_MODEL"], messages=[])\nfinally:')),
            ('a swallowed failure', 'failure: an except handler can swallow a failure', ('finally:', 'except Exception:\n    pass\nfinally:')),
            ('a fallback provider', 'base-url: a literal URL can name another provider',
             ('base_url=os.environ["CHIRAL_BASE_URL"],', 'base_url=os.environ.get("CHIRAL_BASE_URL") or "https://fallback.example/v1",')),
            ('another provider variable', 'base-url: base_url is not exactly os.environ["CHIRAL_BASE_URL"]',
             ('base_url=os.environ["CHIRAL_BASE_URL"],', 'base_url=os.environ["CHIRAL_FALLBACK_URL"],')),
            ('an unclosed stream', 'failure: no finally block closes the stream', ('    stream.close()', '    pass')),
            ('no retry setting (the SDK default retries)', 'retries: the client does not set max_retries=0', ('    max_retries=0,\n', '')),
            ('retries enabled on the client afterwards', 'retries: a retry setting is changed after construction',
             ('stream = client.chat.completions.create(', 'client.max_retries = 2\nstream = client.chat.completions.create(')),
            ('another base URL set on the client', 'base-url: the client or the credentials are changed after construction',
             ('stream = client.chat.completions.create(', 'client.base_url = os.environ["CHIRAL_FALLBACK_URL"]\nstream = client.chat.completions.create(')),
            ('the snippet rerun after the stream', 'retry: a process other than the credential export is started',
             ('import os\n', 'import os\nimport subprocess\nimport sys\n'), ('    stream.close()', '    stream.close()\n    subprocess.run([sys.executable, *sys.argv])')),
        ]
        for label, finding, *edits in cases:
            with self.subTest(label):
                self.check(self.snippet, 'snippet', finding, *edits)


if __name__ == '__main__':
    unittest.main()
