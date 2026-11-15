#!/usr/bin/env node
// Install: npm install openai
// Provision using chiral --config consumer.json discover, quote --offer ... --amount-wei ...,
// fund --offer ... --quote ... (inspect pending and resume QUOTE_ID on any interruption),
// then credentials QUOTE_ID issue. Funding prepares once, persists the provider's signed
// acknowledgement BEFORE broadcast, and resumes the same bytes even after quote expiry.
// A missing acknowledgement never authorizes broadcast; do not replace an uncertain intent.
// Topups reuse the existing immutable agreement, not a new expiring quote.
// Funds must exist on the configured testnet; no fabricated bearer or automatic funding.
// Run: node examples/openai-client.mjs consumer.json QUOTE_ID CREDENTIAL_ID [prompt]
// These are small provider-specific NON-REFUNDABLE balances. No rerouting/retries.
import OpenAI from 'openai';
import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';

const [config, contract, credentialId, prompt = 'Explain prepaid inference in two sentences.'] = process.argv.slice(2);
if (!config || !contract || !credentialId) {
  console.error('Usage: node examples/openai-client.mjs CONSUMER_CONFIG CONTRACT_ID APP_CREDENTIAL_ID [prompt]');
  process.exit(2);
}
let credentials;
try {
  credentials = JSON.parse(execFileSync(process.env.CHIRAL_BIN || 'chiral', [
    '--config', config, 'credentials', contract, 'export', '--id', credentialId,
  ], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 1024 * 1024 }));
} catch {
  console.error('Credential export failed; inspect the account with the CLI. No API request was made.');
  process.exit(1);
}
const client = new OpenAI({ apiKey: credentials.api_key, baseURL: credentials.base_url, maxRetries: 0, timeout: 120_000 });
const requestKey = randomUUID();
const controller = new AbortController();
const cancel = () => controller.abort();
process.once('SIGINT', cancel);
console.error(`Request key: ${requestKey}`);
try {
  const stream = await client.chat.completions.create({
    model: credentials.model,
    messages: [{ role: 'user', content: prompt }],
    max_tokens: 128,
    stream: true,
    stream_options: { include_usage: true },
  }, { headers: { 'Idempotency-Key': requestKey }, signal: controller.signal });
  let receivedUsage = false;
  for await (const chunk of stream) {
    for (const choice of chunk.choices) {
      if (choice.delta.content) process.stdout.write(choice.delta.content);
    }
    if (chunk.usage) {
      receivedUsage = true;
      console.error(`\nUsage: ${JSON.stringify(chunk.usage)}`);
    }
  }
  if (!receivedUsage) throw new Error('Stream ended without authoritative usage');
  process.stdout.write('\n');
  console.error(`Retrieve signed receipt: chiral --config <config> receipt ${contract}`);
} catch {
  console.error('\nGeneration interrupted. Do not automatically replay a paid request.');
  console.error(`Inspect with: chiral --config <config> request ${contract} ${requestKey}`);
  process.exitCode = 1;
} finally {
  controller.abort();
  process.removeListener('SIGINT', cancel);
  credentials.api_key = undefined;
}
