#!/usr/bin/env python3
"""Local-only operator API. Reads bearer from a protected file, never argv.
Run on provider host with --token-file DEPLOY_DIR/secrets/admin.token.
"""
import argparse
import json
from pathlib import Path
import stat
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--token-file', type=Path, required=True)
    parser.add_argument('--port', type=int, default=9421)
    parser.add_argument('action', choices=['status', 'offers', 'accounts', 'fees', 'requests', 'drain', 'resume', 'offers/refresh', 'fees/remit'])
    args = parser.parse_args()
    if not 1 <= args.port <= 65535 or stat.S_IMODE(args.token_file.stat().st_mode) & 0o077:
        parser.error('valid loopback port and protected token file required')
    token = args.token_file.read_text().strip()
    if len(token) < 32 or any(c.isspace() for c in token):
        parser.error('invalid operator token')
    method = 'POST' if args.action in ['drain', 'resume', 'offers/refresh', 'fees/remit'] else 'GET'
    request = urllib.request.Request('http://127.0.0.1:' + str(args.port) + '/admin/' + args.action,
                                     data=b'{}' if method == 'POST' else None,
                                     headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}, method=method)
    # Disable environment proxies; credentials never leave loopback.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(request, timeout=30) as response:
            print(json.dumps(json.load(response), indent=2))
    except urllib.error.HTTPError as error:
        raise SystemExit('operator request rejected: HTTP ' + str(error.code))

if __name__ == '__main__':
    main()
