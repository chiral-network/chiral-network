import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { parse as parseSvelte } from 'svelte/compiler';
import ts from 'typescript';

// inference-request-accounting, requirement *Request identity prevents automatic paid replay*
// (task 2.3): the supplied JavaScript SDK example and the Usage page's JavaScript SDK snippet
// send one paid request and never retry it automatically, with a new key or at another
// provider. Static checks of the parsed source: the `openai` package is not installed here and
// nothing is sent. `scripts/test_sdk_examples.py` checks the Python example and snippet.
// The checks do not show how the SDK itself behaves at run time.

const EXAMPLE = 'examples/openai-client.mjs';
const USAGE = 'src/pages/Usage.svelte';

type Kind = 'example' | 'snippet';

/** The uninterpolated template literal assigned to `name` in Usage.svelte's script. */
function usageSnippet(name: 'javascript' | 'python'): string {
  const ast = parseSvelte(readFileSync(USAGE, 'utf8'), { modern: true });
  const found: string[] = [];
  const visit = (node: unknown): void => {
    if (!node || typeof node !== 'object') return;
    if (Array.isArray(node)) { node.forEach(visit); return; }
    const value = node as Record<string, any>;
    if (value.type === 'VariableDeclarator' && value.id?.type === 'Identifier' && value.id.name === name) {
      const literal = value.init;
      if (literal?.type !== 'TemplateLiteral' || literal.expressions.length !== 0 || literal.quasis.length !== 1) {
        throw new Error(`${name} is not a plain template literal`);
      }
      found.push(literal.quasis[0].value.cooked);
    }
    for (const [key, child] of Object.entries(value)) if (key !== 'parent') visit(child);
  };
  visit(ast.instance?.content);
  if (found.length !== 1) throw new Error(`expected one ${name} snippet, found ${found.length}`);
  return found[0];
}

const propertyName = (name: ts.PropertyName) =>
  ts.isIdentifier(name) || ts.isStringLiteral(name) ? name.text : undefined;
function property(object: ts.Node | undefined, key: string): ts.PropertyAssignment | undefined {
  if (!object || !ts.isObjectLiteralExpression(object)) return undefined;
  return object.properties.find((p): p is ts.PropertyAssignment => ts.isPropertyAssignment(p) && propertyName(p.name) === key);
}
function descendants(root: ts.Node): ts.Node[] {
  const nodes: ts.Node[] = [];
  const collect = (node: ts.Node) => { nodes.push(node); ts.forEachChild(node, collect); };
  collect(root);
  return nodes;
}
const stringPiece = (node: ts.Node): string | undefined =>
  ts.isStringLiteralLike(node) || ts.isTemplateHead(node) || ts.isTemplateMiddle(node) || ts.isTemplateTail(node) ? node.text : undefined;

/**
 * Every way `source` departs from a single, never-retried paid request; empty when it has none.
 * `example` is the standalone script, which must also generate, print and send one
 * Idempotency-Key, take its base URL from the explicit credential export and, on failure, exit
 * non-zero naming the key to inspect. `snippet` is the Usage page's environment-based snippet,
 * which has no key and no catch clause, so a failure propagates.
 */
function audit(source: string, kind: Kind): string[] {
  const file = ts.createSourceFile('client.mjs', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  const syntax = (file as unknown as { parseDiagnostics: ts.Diagnostic[] }).parseDiagnostics;
  if (syntax.length) return ['syntax: the source does not parse'];
  const findings = new Set<string>();
  const nodes = descendants(file);
  const calls = nodes.filter(ts.isCallExpression);
  const text = (node: ts.Node | undefined) => node?.getText(file);
  const callee = (call: ts.CallExpression) =>
    ts.isPropertyAccessExpression(call.expression) ? call.expression.name.text : ts.isIdentifier(call.expression) ? call.expression.text : undefined;

  // Only the SDK sends: no other HTTP client or retry helper.
  const transports = new Set(['http', 'https', 'node:http', 'node:https', 'node:net', 'undici', 'axios', 'node-fetch', 'got', 'ky', 'p-retry', 'async-retry', 'retry',
    'worker_threads', 'node:worker_threads', 'cluster', 'node:cluster', 'vm', 'node:vm']);
  for (const declaration of nodes.filter(ts.isImportDeclaration)) {
    const module = (declaration.moduleSpecifier as ts.StringLiteral).text;
    if (transports.has(module)) findings.add(`transport: imports ${module}`);
  }
  if (calls.some(call => callee(call) === 'fetch')) findings.add('transport: calls fetch');

  // One client, with SDK retries disabled.
  const clients = nodes.filter(ts.isNewExpression).filter(node => ts.isIdentifier(node.expression) && node.expression.text === 'OpenAI');
  if (clients.length !== 1) findings.add(`retries: ${clients.length} OpenAI clients, expected one`);
  const options = clients[0]?.arguments?.[0];
  const retries = property(options, 'maxRetries');
  if (!retries || !ts.isNumericLiteral(retries.initializer) || retries.initializer.text !== '0') {
    findings.add('retries: the client does not set maxRetries: 0');
  }
  if (nodes.some(node => (ts.isPropertyAssignment(node) || ts.isShorthandPropertyAssignment(node)) && propertyName(node.name) === 'maxRetries' && node !== retries)) {
    findings.add('retries: another maxRetries setting');
  }
  if (calls.some(call => callee(call) === 'withOptions')) findings.add('retries: a client copy with other options');
  if (nodes.some(ts.isSpreadAssignment)) findings.add('retries: an object spread can carry other client or request options');

  // Nothing changes the client or the exported credentials after construction: the SDK
  // client's `maxRetries` and `baseURL` can be reassigned, and a value written into the
  // credentials would replace the exported base URL. Only the exit status and the final
  // clearing of the exported key are written.
  const written = (target: string) => findings.add(/retr/i.test(target)
    ? 'retries: a retry setting is changed after construction'
    : 'base-url: the client or the credentials are changed after construction');
  for (const node of nodes) {
    if (ts.isBinaryExpression(node) && node.operatorToken.kind >= ts.SyntaxKind.FirstAssignment
      && node.operatorToken.kind <= ts.SyntaxKind.LastAssignment
      && (ts.isPropertyAccessExpression(node.left) || ts.isElementAccessExpression(node.left))) {
      const target = text(node.left) ?? '';
      const allowed = target === 'process.exitCode'
        || (target === 'credentials.api_key' && node.operatorToken.kind === ts.SyntaxKind.EqualsToken && text(node.right) === 'undefined');
      if (!allowed) written(target);
    }
    if (ts.isDeleteExpression(node)) written(text(node.expression) ?? '');
    if (ts.isCallExpression(node) && /^(Object\.(assign|defineProperty|defineProperties|setPrototypeOf)|Reflect\.(set|defineProperty))$/.test(text(node.expression) ?? '')) {
      written(text(node) ?? '');
    }
  }

  // No process other than the explicit credential export, and no dynamic code, can run the
  // request again (for example by re-running the script after a failure).
  const processes = calls.filter(call => ['execFileSync', 'execSync', 'spawn', 'spawnSync', 'fork', 'exec', 'execFile'].includes(callee(call) ?? ''));
  const exports = processes.filter(call => text(call.expression) === 'execFileSync' && call.arguments[1] && ts.isArrayLiteralExpression(call.arguments[1])
    && ['credentials', 'export'].every(word => (call.arguments[1] as ts.ArrayLiteralExpression).elements.some(element => stringPiece(element) === word)));
  if (processes.length !== (kind === 'example' ? 1 : 0) || exports.length !== processes.length) {
    findings.add('retry: a process other than the credential export is started');
  }
  if (calls.some(call => call.expression.kind === ts.SyntaxKind.ImportKeyword || ['eval', 'Function'].includes(callee(call) ?? ''))
    || nodes.some(node => ts.isNewExpression(node) && text(node.expression) === 'Function')) {
    findings.add('transport: dynamic import or code execution');
  }

  // Exactly one paid call, made once: not in a loop, a re-callable function, a catch or a finally.
  const creates = calls.filter(call => ts.isPropertyAccessExpression(call.expression) && call.expression.name.text === 'create');
  if (creates.length !== 1) findings.add(`one-call: ${creates.length} create calls, expected one`);
  const create = creates[0];
  if (create && text(create.expression) !== 'client.chat.completions.create') findings.add('one-call: the call is not client.chat.completions.create');
  const path: ts.Node[] = [];
  for (let node: ts.Node | undefined = create; node; node = node.parent) path.push(node);
  for (let index = 1; index < path.length; index++) {
    const [ancestor, child] = [path[index], path[index - 1]];
    if (ts.isIterationStatement(ancestor, false)) findings.add('loop: the create call is inside a loop');
    if (ts.isFunctionLike(ancestor)) findings.add('loop: the create call is inside a function that can run again');
    if (ts.isCatchClause(ancestor)) findings.add('retry: the create call is inside a catch clause');
    if (ts.isTryStatement(ancestor) && child === ancestor.finallyBlock) findings.add('retry: the create call is inside a finally block');
  }

  // The base URL (and, for the snippet, key and model) come only from the exported credential.
  const base = property(options, 'baseURL')?.initializer;
  if (nodes.some(node => stringPiece(node)?.includes('://'))) findings.add('base-url: a literal URL can name another provider');
  if (kind === 'snippet') {
    if (text(base) !== 'process.env.CHIRAL_BASE_URL') findings.add('base-url: the base URL is not exactly CHIRAL_BASE_URL');
    if (text(property(options, 'apiKey')?.initializer) !== 'process.env.CHIRAL_API_KEY') findings.add('base-url: the key is not exactly CHIRAL_API_KEY');
    if (text(property(create?.arguments[0], 'model')?.initializer) !== 'process.env.CHIRAL_MODEL') findings.add('base-url: the model is not exactly CHIRAL_MODEL');
  } else {
    if (text(base) !== 'credentials.base_url') findings.add('base-url: the base URL is not the exported credential\'s base_url');
    const assignments = nodes.filter(node => ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken
      && ts.isIdentifier(node.left) && node.left.text === 'credentials') as ts.BinaryExpression[];
    const declarations = nodes.filter(node => ts.isVariableDeclaration(node) && text(node.name) === 'credentials') as ts.VariableDeclaration[];
    const exported = assignments[0]?.right;
    const exporter = exported && ts.isCallExpression(exported) && text(exported.expression) === 'JSON.parse' ? exported.arguments[0] : undefined;
    const argv = exporter && ts.isCallExpression(exporter) && text(exporter.expression) === 'execFileSync' ? exporter.arguments[1] : undefined;
    const words = argv && ts.isArrayLiteralExpression(argv) ? argv.elements.map(element => stringPiece(element)) : [];
    if (assignments.length !== 1 || declarations.length !== 1 || declarations[0].initializer || !words.includes('credentials') || !words.includes('export')) {
      findings.add('base-url: credentials are not assigned once from the explicit credential export');
    }
  }

  if (kind === 'example') {
    // One key, generated once before the call, printed for reconciliation and sent with the call.
    const generated = calls.filter(call => callee(call) === 'randomUUID');
    if (generated.length !== 1) findings.add(`key: ${generated.length} generated keys, expected one`);
    const declaration = generated[0]?.parent;
    const once = declaration && ts.isVariableDeclaration(declaration) && declaration.initializer === generated[0]
      && ts.isIdentifier(declaration.name) && (declaration.parent.flags & ts.NodeFlags.Const) !== 0
      && ts.isVariableStatement(declaration.parent.parent) && declaration.parent.parent.parent === file
      && create !== undefined && declaration.end < create.pos;
    if (!once) findings.add('key: the key is not one top-level const generated before the call');
    const keyName = declaration && ts.isVariableDeclaration(declaration) ? text(declaration.name) : undefined;
    const header = property(property(create?.arguments[1], 'headers')?.initializer, 'Idempotency-Key')?.initializer;
    if (!header || !ts.isIdentifier(header) || header.text !== keyName) findings.add('key: the call does not send the generated key as its Idempotency-Key');
    if (nodes.filter(node => stringPiece(node)?.toLowerCase() === 'idempotency-key').length !== 1) findings.add('key: another Idempotency-Key');
    const mentions = (root: ts.Node) => descendants(root).filter(ts.isTemplateExpression)
      .filter(template => template.templateSpans.some(span => text(span.expression) === keyName));
    if (!create || !mentions(file).some(template => template.end < create.pos && template.head.text.includes('Request key'))) {
      findings.add('key: the key is not printed before the call');
    }

    // On failure: no second call, a non-zero exit and the key to inspect.
    const guard = path.find((node, index) => index > 0 && ts.isTryStatement(node) && path[index - 1] === node.tryBlock) as ts.TryStatement | undefined;
    if (!guard?.catchClause) {
      findings.add('failure: the call is not inside a try block with a catch clause');
    } else {
      const handler = guard.catchClause.block;
      const inside = descendants(handler);
      const nonZero = (node: ts.Node | undefined) => !!node && ts.isNumericLiteral(node) && Number(node.text) !== 0;
      const exits = inside.some(node => ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken
        && text(node.left) === 'process.exitCode' && nonZero(node.right))
        || inside.some(node => ts.isCallExpression(node) && text(node.expression) === 'process.exit' && nonZero(node.arguments[0]));
      if (!exits) findings.add('failure: the catch clause does not set a non-zero exit status');
      if (!inside.some(node => stringPiece(node)?.includes('Do not automatically replay'))) findings.add('failure: the catch clause does not warn against replay');
      if (!mentions(handler).some(template => template.head.text.includes('Inspect with') && template.getText(file).includes(' request '))) {
        findings.add('failure: the catch clause does not name the request key to inspect');
      }
    }
  } else {
    // The snippet catches nothing, so a failure propagates; its finally only stops the stream.
    if (nodes.some(ts.isCatchClause)) findings.add('failure: a catch clause can swallow a failure');
    const stops = nodes.filter(ts.isTryStatement).some(statement => statement.finallyBlock
      && descendants(statement.finallyBlock).some(node => ts.isCallExpression(node) && text(node.expression) === 'stream.controller.abort'));
    if (!stops) findings.add('failure: no finally block stops the stream');
  }
  return [...findings];
}

/** `source` with the single occurrence of each `from` replaced by its `to`. */
function mutate(source: string, ...edits: [string, string][]): string {
  for (const [from, to] of edits) {
    expect(source.split(from).length - 1, `mutation anchor ${JSON.stringify(from)}`).toBe(1);
    source = source.replace(from, () => to);
  }
  return source;
}

describe('supplied JavaScript SDK clients never retry paid inference automatically', () => {
  const example = readFileSync(EXAMPLE, 'utf8');
  const snippet = usageSnippet('javascript');

  it('the example sends one request with retries off, one printed key and the exported base URL, and exits non-zero on failure', () => {
    expect(audit(example, 'example')).toEqual([]);
  });

  it('the Usage page snippet sends one request with retries off from the environment and lets a failure propagate', () => {
    expect(audit(snippet, 'snippet')).toEqual([]);
  });

  // Each check is shown to fail on a mutated copy of the real source.
  const examples: [string, string, string, string][] = [
    ['SDK retries enabled', 'maxRetries: 0', 'maxRetries: 2', 'retries: the client does not set maxRetries: 0'],
    ['a per-request retry setting', "signal: controller.signal }", 'signal: controller.signal, maxRetries: 2 }', 'retries: another maxRetries setting'],
    ['a fetch fallback', "process.exitCode = 1;", "process.exitCode = 1;\n  await fetch(credentials.base_url);", 'transport: calls fetch'],
    ['a loop around the call', '\ntry {\n  const stream = await client', '\nfor (const attempt of [1, 2]) try {\n  const stream = await client', 'loop: the create call is inside a loop'],
    ['a second call in the catch clause', "} catch {\n  console.error('\\nGeneration", "} catch {\n  await client.chat.completions.create({ model: credentials.model, messages: [] }, { headers: { 'Idempotency-Key': randomUUID() } });\n  console.error('\\nGeneration",
      'one-call: 2 create calls, expected one'],
    ['a new key per attempt', "headers: { 'Idempotency-Key': requestKey }", "headers: { 'Idempotency-Key': randomUUID() }", 'key: 2 generated keys, expected one'],
    ['a key that is not sent', "headers: { 'Idempotency-Key': requestKey }", "headers: { 'Idempotency-Key': 'fixed' }", 'key: the call does not send the generated key as its Idempotency-Key'],
    ['a key that is not printed', 'console.error(`Request key: ${requestKey}`);', '', 'key: the key is not printed before the call'],
    ['a fallback provider', 'baseURL: credentials.base_url', "baseURL: credentials.base_url ?? 'https://fallback.example/v1'", 'base-url: a literal URL can name another provider'],
    ['a base URL from the environment', 'baseURL: credentials.base_url', 'baseURL: process.env.OTHER_PROVIDER', "base-url: the base URL is not the exported credential's base_url"],
    ['a zero exit status on failure', 'process.exitCode = 1;', 'process.exitCode = 0;', 'failure: the catch clause does not set a non-zero exit status'],
    ['no key to inspect', "console.error(`Inspect with: chiral --config <config> request ${contract} ${requestKey}`);", '', 'failure: the catch clause does not name the request key to inspect'],
    ['no replay warning', "console.error('\\nGeneration interrupted. Do not automatically replay a paid request.');", '', 'failure: the catch clause does not warn against replay'],
    // The SDKs retry twice by default, so an omitted setting must fail too.
    ['no retry setting (the SDK default retries)', ', maxRetries: 0', '', 'retries: the client does not set maxRetries: 0'],
    ['retries enabled on the client afterwards', 'const requestKey = randomUUID();', 'client.maxRetries = 2;\nconst requestKey = randomUUID();', 'retries: a retry setting is changed after construction'],
    ['spread client options', 'maxRetries: 0, timeout: 120_000', 'maxRetries: 0, ...credentials.options, timeout: 120_000', 'retries: an object spread can carry other client or request options'],
    ['spread request options', 'signal: controller.signal }', 'signal: controller.signal, ...credentials.request_options }', 'retries: an object spread can carry other client or request options'],
    ['another base URL set on the client', 'const requestKey = randomUUID();', 'client.baseURL = process.env.CHIRAL_FALLBACK_URL;\nconst requestKey = randomUUID();',
      'base-url: the client or the credentials are changed after construction'],
    ['a fallback written into the credentials', 'const client = new OpenAI', 'credentials.base_url ??= process.env.CHIRAL_FALLBACK_URL;\nconst client = new OpenAI',
      'base-url: the client or the credentials are changed after construction'],
    ['credentials assigned another base URL', 'const client = new OpenAI', 'Object.assign(credentials, { base_url: process.env.CHIRAL_FALLBACK_URL });\nconst client = new OpenAI',
      'base-url: the client or the credentials are changed after construction'],
    ['the script rerun from the catch clause', '  process.exitCode = 1;\n', "  process.exitCode = 1;\n  execFileSync(process.execPath, process.argv.slice(1), { stdio: 'inherit' });\n",
      'retry: a process other than the credential export is started'],
  ];
  it.each(examples)('the example check catches %s', (_label, from, to, finding) => {
    const findings = audit(mutate(example, [from, to]), 'example');
    expect(findings).not.toContain('syntax: the source does not parse');
    expect(findings).toContain(finding);
  });

  it('the example check catches a re-callable attempt', () => {
    const findings = audit(mutate(example,
      ['  const stream = await client.chat.completions.create({', '  const attempt = () => client.chat.completions.create({'],
      ['  let receivedUsage = false;', '  const stream = await attempt();\n  let receivedUsage = false;']), 'example');
    expect(findings).not.toContain('syntax: the source does not parse');
    expect(findings).toContain('loop: the create call is inside a function that can run again');
  });

  const snippets: [string, string, string, string][] = [
    ['SDK retries enabled', 'maxRetries: 0,', 'maxRetries: 3,', 'retries: the client does not set maxRetries: 0'],
    ['a loop around the call', 'const stream = await client', 'let stream; for (const attempt of [1, 2]) stream = await client', 'loop: the create call is inside a loop'],
    ['a second call after a caught failure', '} finally {', '} catch {\n  await client.chat.completions.create({ model: process.env.CHIRAL_MODEL, messages: [] });\n} finally {',
      'one-call: 2 create calls, expected one'],
    ['a swallowed failure', '} finally {', '} catch {\n} finally {', 'failure: a catch clause can swallow a failure'],
    ['a fallback provider', 'baseURL: process.env.CHIRAL_BASE_URL,', 'baseURL: process.env.CHIRAL_BASE_URL ?? "https://fallback.example/v1",', 'base-url: a literal URL can name another provider'],
    ['another provider variable', 'baseURL: process.env.CHIRAL_BASE_URL,', 'baseURL: process.env.CHIRAL_FALLBACK_URL,', 'base-url: the base URL is not exactly CHIRAL_BASE_URL'],
    ['an unstopped stream', 'stream.controller.abort();', '', 'failure: no finally block stops the stream'],
    ['no retry setting (the SDK default retries)', '  maxRetries: 0,\n', '', 'retries: the client does not set maxRetries: 0'],
    ['retries enabled on the client afterwards', 'const stream = await client', 'client.maxRetries = 2;\nconst stream = await client', 'retries: a retry setting is changed after construction'],
    ['another base URL set on the client', 'const stream = await client', 'client.baseURL = process.env.CHIRAL_FALLBACK_URL;\nconst stream = await client',
      'base-url: the client or the credentials are changed after construction'],
    ['spread client options', '  maxRetries: 0,\n', '  maxRetries: 0,\n  ...JSON.parse(process.env.CHIRAL_CLIENT_OPTIONS ?? "{}"),\n', 'retries: an object spread can carry other client or request options'],
  ];
  it.each(snippets)('the snippet check catches %s', (_label, from, to, finding) => {
    const findings = audit(mutate(snippet, [from, to]), 'snippet');
    expect(findings).not.toContain('syntax: the source does not parse');
    expect(findings).toContain(finding);
  });
});
