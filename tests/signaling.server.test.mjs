import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'child_process';
import WebSocket from 'ws';
import path from 'path';

const SERVER_PATH = path.resolve('src/lib/services/server/server.cjs');

function wait(ms) { return new Promise(r => setTimeout(r, ms)); }

async function startServer() {
  const node = spawn(process.execPath, [SERVER_PATH], { stdio: ['ignore', 'pipe', 'pipe'] });
  node.stdout.on('data', d => process.stdout.write(`[server] ${d}`));
  node.stderr.on('data', d => process.stderr.write(`[server] ${d}`));
  await wait(200);
  return node;
}

async function stopServer(node) {
  return new Promise((resolve) => {
    node.on('exit', () => resolve());
    node.kill();
  });
}

test('server should register peers and broadcast peer list', async () => {
  const server = await startServer();

  const a = new WebSocket('ws://localhost:9000');
  const b = new WebSocket('ws://localhost:9000');

  await Promise.all([
    new Promise(r => a.on('open', r)),
    new Promise(r => b.on('open', r))
  ]);

  let aPeers = null;
  let bPeers = null;

  a.on('message', data => {
    const msg = JSON.parse(data.toString());
    if (msg.type === 'peers') aPeers = msg.peers;
  });
  b.on('message', data => {
    const msg = JSON.parse(data.toString());
    if (msg.type === 'peers') bPeers = msg.peers;
  });

  // Register both
  a.send(JSON.stringify({ type: 'register', clientId: 'A' }));
  b.send(JSON.stringify({ type: 'register', clientId: 'B' }));

  // wait
  await wait(300);

  assert.ok(Array.isArray(aPeers));
  assert.ok(Array.isArray(bPeers));
  // Each should be aware of the other (server sends peers excluding self on initial register)
  assert.ok(aPeers.includes('B') || bPeers.includes('A'));

  a.close(); b.close();
  await stopServer(server);
});

test('server routes direct messages using `to` field', async () => {
  const server = await startServer();

  const a = new WebSocket('ws://localhost:9000');
  const b = new WebSocket('ws://localhost:9000');

  await Promise.all([
    new Promise(r => a.on('open', r)),
    new Promise(r => b.on('open', r))
  ]);

  // Register
  a.send(JSON.stringify({ type: 'register', clientId: 'A2' }));
  b.send(JSON.stringify({ type: 'register', clientId: 'B2' }));

  let received = null;
  b.on('message', d => {
    const m = JSON.parse(d.toString());
    if (m.message && m.message.test === 'hello') received = m;
  });

  await wait(200);

  // Send direct message A -> B
  a.send(JSON.stringify({ type: 'message', to: 'B2', from: 'A2', message: { test: 'hello' } }));

  await wait(200);

  assert.ok(received, 'B did not receive direct message');
  assert.equal(received.message.test, 'hello');

  a.close(); b.close();
  await stopServer(server);
});

test('server falls back to broadcast when no `to` field', async () => {
  const server = await startServer();

  const a = new WebSocket('ws://localhost:9000');
  const b = new WebSocket('ws://localhost:9000');
  const c = new WebSocket('ws://localhost:9000');

  await Promise.all([
    new Promise(r => a.on('open', r)),
    new Promise(r => b.on('open', r)),
    new Promise(r => c.on('open', r)),
  ]);

  a.send(JSON.stringify({ type: 'register', clientId: 'A3' }));
  b.send(JSON.stringify({ type: 'register', clientId: 'B3' }));
  c.send(JSON.stringify({ type: 'register', clientId: 'C3' }));

  let bGot = false;
  let cGot = false;

  b.on('message', d => { const m = JSON.parse(d.toString()); if (m.broadcast === 'ping') bGot = true; });
  c.on('message', d => { const m = JSON.parse(d.toString()); if (m.broadcast === 'ping') cGot = true; });

  await wait(200);

  a.send(JSON.stringify({ type: 'broadcast', broadcast: 'ping' }));

  await wait(200);

  assert.ok(bGot && cGot, 'Broadcast did not reach peers');

  a.close(); b.close(); c.close();
  await stopServer(server);
});

test('server updates peer list on disconnect', async () => {
  const server = await startServer();

  const a = new WebSocket('ws://localhost:9000');
  const b = new WebSocket('ws://localhost:9000');

  await Promise.all([
    new Promise(r => a.on('open', r)),
    new Promise(r => b.on('open', r))
  ]);

  a.send(JSON.stringify({ type: 'register', clientId: 'A4' }));
  b.send(JSON.stringify({ type: 'register', clientId: 'B4' }));

  let latestPeers = null;
  b.on('message', d => { const m = JSON.parse(d.toString()); if (m.type === 'peers') latestPeers = m.peers; });

  await wait(200);
  // Close A, server should notify B
  a.close();
  await wait(300);

  assert.ok(Array.isArray(latestPeers));
  assert.ok(!latestPeers.includes('A4'));

  b.close();
  await stopServer(server);
});
