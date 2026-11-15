#!/usr/bin/env python3
"""Real paid SDK qualification; never substitutes a mock or silently skips a gate.

Requires Python openai/httpx packages, built chiral CLI, funded wallet/config,
real signed offer, live qualified chain and llama.cpp, and an ISOLATED deployment.
Funds are non-refundable. --restart-env authorizes SIGKILL of that Compose
provider during active generation; do not point this at a shared/live service.
Outputs measurements and accounting evidence only, never keys or model text.
"""
import argparse
import concurrent.futures
import json
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time
import uuid
import httpx
from openai import OpenAI, APIStatusError


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--cli', required=True)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--offer', type=Path, required=True)
    parser.add_argument('--funding-wei', required=True)
    parser.add_argument('--restart-env', type=Path, required=True)
    parser.add_argument('--compose', type=Path, required=True)
    parser.add_argument('--allow-isolated-restart', action='store_true', required=True)
    parser.add_argument('--requests', type=int, default=8)
    parser.add_argument('--concurrency', type=int, default=2)
    parser.add_argument('--settlement-timeout', type=int, default=600)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    require(args.funding_wei.isdecimal() and int(args.funding_wei) > 0, 'explicit nonzero test funding required')
    require(1 <= args.concurrency <= args.requests <= 64, 'require 1 <= concurrency <= requests <= 64')
    require(not args.report.exists(), 'report exists; retain each run separately')
    for path in [args.config, args.offer, args.restart_env, args.compose]:
        require(path.is_file(), 'required input file absent')
    cli_base = [args.cli, '--config', str(args.config)]
    def cli(*command, allow_failure=False):
        result = subprocess.run(cli_base + list(command), capture_output=True, text=True, timeout=args.settlement_timeout)
        if result.returncode:
            if allow_failure:
                return None
            raise RuntimeError('chiral command failed: ' + command[0] + '; inspect private CLI diagnostics separately')
        return json.loads(result.stdout)
    compose = ['docker', 'compose', '--env-file', str(args.restart_env), '-f', str(args.compose)]
    subprocess.run(compose + ['ps', '--status', 'running', '--services'], check=True, capture_output=True, timeout=30)
    cli('network', 'status')
    cli('wallet', 'balance')
    offer = json.loads(args.offer.read_text())
    require({'chat', 'completions'} <= set(offer['payload']['model']['capabilities']), 'reference workload requires advertised chat and completions')
    quote = cli('quote', '--offer', str(args.offer), '--amount-wei', args.funding_wei)
    contract = quote['payload']['id']
    with tempfile.TemporaryDirectory(prefix='chiral-qualification-') as directory:
        quote_path = Path(directory) / 'quote.json'
        quote_path.write_text(json.dumps(quote))
        funded = cli('fund', '--offer', str(args.offer), '--quote', str(quote_path), allow_failure=True)
    deadline = time.monotonic() + args.settlement_timeout
    while funded is None and time.monotonic() < deadline:
        time.sleep(3)
        funded = cli('resume', contract, allow_failure=True)
    require(funded is not None, 'original funding not settled; use chiral resume ' + contract + ', never re-fund automatically')
    baseline = cli('account', contract)
    require(int(baseline['funded_wei']) == int(args.funding_wei), 'funding amount mismatch')
    replay = cli('resume', contract)
    require(replay['funded_wei'] == baseline['funded_wei'], 'opening replay duplicated credit')
    credentials = cli('credentials', contract, 'export')
    origin = credentials['base_url'].removesuffix('/v1').rstrip('/')
    model = credentials['model']
    sdk = OpenAI(api_key=credentials['api_key'], base_url=credentials['base_url'], max_retries=0, timeout=120)
    http = httpx.Client(timeout=10, follow_redirects=False, trust_env=False)
    headers = {'Authorization': 'Bearer ' + credentials['api_key']}
    require(http.get(origin + '/readyz').status_code == 200, 'provider not ready')
    for path in ['/admin/status', '/admin/runtime', '/tokenize', '/apply-template', '/props', '/slots', '/metrics', '/api/headless/wallet']:
        require(http.get(origin + path, headers=headers).status_code in (401, 403, 404, 405), 'private/retired endpoint publicly reachable: ' + path)
    unauthorized = http.post(origin + '/v1/chat/completions', json={'model': model, 'messages': [{'role': 'user', 'content': 'hello'}], 'max_tokens': 8})
    require(unauthorized.status_code == 401, 'inference accepted missing bearer')
    request_ids = []
    expected_costs = {}
    input_price = int(baseline['price']['input_wei_per_1k'])
    output_price = int(baseline['price']['output_wei_per_1k'])
    def cost(usage):
        return (usage.prompt_tokens * input_price + usage.completion_tokens * output_price + 999) // 1000
    def new_id():
        value = 'qualification-' + uuid.uuid4().hex
        request_ids.append(value)
        return value
    def settled(request_id, timeout=120):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            response = http.get(origin + '/v1/contracts/' + contract + '/requests/' + request_id, headers=headers)
            if response.status_code == 200:
                row = response.json()
                if row['status'] != 'reserved':
                    return row
            elif response.status_code not in (404, 503):
                raise RuntimeError('request reconciliation failed with HTTP ' + str(response.status_code))
            time.sleep(1)
        raise RuntimeError('request did not settle: ' + request_id)
    def complete(kind='chat'):
        request_id = new_id()
        start = time.monotonic()
        if kind == 'chat':
            response = sdk.chat.completions.create(model=model, messages=[{'role': 'user', 'content': 'Give a short explanation of prepaid accounting.'}], max_tokens=32, extra_headers={'Idempotency-Key': request_id})
        else:
            response = sdk.completions.create(model=model, prompt='Prepaid accounting means', max_tokens=32, extra_headers={'Idempotency-Key': request_id})
        require(response.usage is not None and response.usage.prompt_tokens > 0 and response.usage.completion_tokens > 0, 'engine omitted actual usage')
        expected_costs[request_id] = cost(response.usage)
        row = settled(request_id)
        require(int(row['cost_wei']) == expected_costs[request_id], 'request cost differs from exact SDK usage')
        return time.monotonic() - start
    complete('completions')
    complete()
    stream_id = new_id()
    stream = sdk.chat.completions.create(model=model, messages=[{'role': 'user', 'content': 'Explain reservations in two sentences.'}], max_tokens=48, stream=True, stream_options={'include_usage': True}, extra_headers={'Idempotency-Key': stream_id})
    usage = None
    chunks = 0
    for chunk in stream:
        chunks += 1
        if chunk.usage is not None:
            usage = chunk.usage
    stream.close()
    require(chunks > 1 and usage is not None and usage.completion_tokens > 0, 'SSE did not deliver real final usage')
    require(int(settled(stream_id)['cost_wei']) == cost(usage), 'SSE charge mismatch')
    cancel_id = new_id()
    stream = sdk.chat.completions.create(model=model, messages=[{'role': 'user', 'content': 'Write a long numbered list of accounting examples.'}], max_tokens=min(512, baseline['model']['max_output_tokens']), stream=True, extra_headers={'Idempotency-Key': cancel_id})
    next(iter(stream))
    stream.close()
    cancelled = settled(cancel_id)
    require(cancelled['status'] != 'reserved', 'cancelled stream retained reservation')
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        latencies = list(pool.map(lambda _: complete(), range(args.requests)))
    restart_id = new_id()
    stream = sdk.chat.completions.create(model=model, messages=[{'role': 'user', 'content': 'Write an extensive numbered list of unique accounting examples, continuing until the limit.'}], max_tokens=baseline['model']['max_output_tokens'], stream=True, extra_headers={'Idempotency-Key': restart_id})
    next(iter(stream))
    active = http.get(origin + '/v1/contracts/' + contract + '/requests/' + restart_id, headers=headers)
    require(active.status_code == 200 and active.json()['status'] == 'reserved', 'restart injection missed active reservation; no crash qualification possible')
    try:
        subprocess.run(compose + ['kill', '--signal', 'SIGKILL', 'provider'], check=True, capture_output=True, timeout=30)
    finally:
        stream.close()
        subprocess.run(compose + ['up', '-d', '--no-deps', 'provider'], check=True, capture_output=True, timeout=60)
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        try:
            if http.get(origin + '/readyz').status_code == 200:
                break
        except httpx.TransportError:
            pass
        time.sleep(2)
    else:
        raise RuntimeError('provider failed readiness after restart')
    recovered = settled(restart_id)
    require(recovered['status'] != 'reserved', 'restart retained in-flight reservation')
    complete()
    account = cli('account', contract)
    receipt = cli('receipt', contract)  # CLI cryptographically verifies before returning.
    require(receipt['payload']['account'] == account, 'signed receipt does not reconcile with account')
    require(int(account['reserved_wei']) == 0, 'leaked reservations')
    require(int(account['funded_wei']) - int(account['fee_wei']) - int(account['spent_wei']) - int(account['reversed_wei']) == int(account['available_wei']), 'account conservation failed')
    rows = [settled(request_id) for request_id in request_ids]
    require(sum(int(row['cost_wei']) for row in rows) == int(account['spent_wei']) - int(baseline['spent_wei']), 'request ledger sum differs from charged account')
    cli('credentials', contract, 'revoke', credentials['credential_id'])
    try:
        sdk.chat.completions.create(model=model, messages=[{'role': 'user', 'content': 'must reject'}], max_tokens=1)
    except APIStatusError as error:
        require(error.status_code in (401, 403), 'revoked credential returned wrong error')
    else:
        raise RuntimeError('revoked credential still authorizes inference')
    report = {'contract_id': contract, 'network': account['network'], 'model': account['model'], 'requests': len(rows), 'concurrency': args.concurrency, 'load_latency_seconds': {'min': min(latencies), 'median': statistics.median(latencies), 'max': max(latencies)}, 'account': account, 'receipt': receipt, 'cancel_status': cancelled['status'], 'restart_status': recovered['status']}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    with args.report.open('x') as output:
        json.dump(report, output, indent=2)
    sdk.close()
    http.close()
    print('Real SDK/accounting/restart evidence written to ' + str(args.report))

if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # Only our fixed diagnostic strings are safe; SDK exceptions can contain model text.
        detail = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        print('Qualification failed: ' + detail + '; no passing evidence produced.', file=sys.stderr)
        sys.exit(1)
