#!/usr/bin/env python3
"""Standard OpenAI SDK against a CLI-provisioned Chiral account.

Install: python -m pip install openai
Provision with the actual CLI (JSON outputs may be saved by your shell):
  chiral --config consumer.json wallet create
  chiral --config consumer.json network status
  chiral --config consumer.json discover --model YOUR_MODEL
  # Save one verified signed offer as offer.json; fund the wallet using the network faucet.
  chiral --config consumer.json quote --offer offer.json --amount-wei YOUR_SMALL_PREPAYMENT
  # Save the returned signed quote as quote.json. Inspect its immutable terms first.
  chiral --config consumer.json fund --offer offer.json --quote quote.json
  # Funding prepares once, registers a signed intent, then broadcasts only after its
  # provider acknowledgement is verified and persisted. Accepted terms survive quote expiry.
  # On any interruption inspect: chiral --config consumer.json pending
  # Resume registration/settlement: chiral --config consumer.json resume QUOTE_ID
  # Never replace an uncertain intent. Topups bind the existing immutable agreement.
  chiral --config consumer.json credentials QUOTE_ID issue
  python examples/openai_client.py --config consumer.json --contract QUOTE_ID --credential CREDENTIAL_ID

Prepayments are provider-specific and non-refundable. Credential export is explicit,
kept in this process, and never printed. No automatic retries or provider rerouting.
"""
import argparse
import json
import subprocess
import sys
import uuid

from openai import OpenAI


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    parser.add_argument("--contract", required=True)
    parser.add_argument("--credential", required=True, help="separate app credential ID from CLI issue")
    parser.add_argument("--chiral", default="chiral")
    parser.add_argument("--prompt", default="Explain what a prepaid inference account is in two sentences.")
    args = parser.parse_args()
    result = subprocess.run(
        [args.chiral, "--config", args.config, "credentials", args.contract, "export", "--id", args.credential],
        capture_output=True, check=False, text=True,
    )
    if result.returncode:
        raise SystemExit("Credential export failed; inspect the account with the CLI. No API request was made.")
    credentials = json.loads(result.stdout)
    request_key = str(uuid.uuid4())
    print(f"Request key: {request_key}", file=sys.stderr)
    try:
        with OpenAI(api_key=credentials["api_key"], base_url=credentials["base_url"], max_retries=0, timeout=120.0) as client:
            with client.chat.completions.create(
                model=credentials["model"],
                messages=[{"role": "user", "content": args.prompt}],
                max_tokens=128,
                stream=True,
                stream_options={"include_usage": True},
                extra_headers={"Idempotency-Key": request_key},
            ) as stream:
                received_usage = False
                for chunk in stream:
                    for choice in chunk.choices:
                        if choice.delta.content:
                            print(choice.delta.content, end="", flush=True)
                    if chunk.usage is not None:
                        received_usage = True
                        print("\nUsage: " + chunk.usage.model_dump_json(), file=sys.stderr)
                if not received_usage:
                    raise RuntimeError("stream ended without authoritative usage")
        print()
    except (KeyboardInterrupt, Exception):
        # Closing the SDK response cancels upstream generation. Settlement remains authoritative.
        print("\nGeneration interrupted. Do not automatically replay this paid request.", file=sys.stderr)
        print(f"Inspect with: chiral --config <config> request {args.contract} {request_key}", file=sys.stderr)
        return 1
    finally:
        credentials.clear()
    print(f"Retrieve the signed receipt: chiral --config <config> receipt {args.contract}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
