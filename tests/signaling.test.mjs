import { spawn } from 'child_process';
import WebSocket from 'ws';
import path from 'path';

function wait(ms) { return new Promise(r => setTimeout(r, ms)); }

async function startServer() {
  const serverPath = path.resolve('src/lib/services/server/server.cjs');
  const node = spawn(process.execPath, [serverPath], { stdio: ['ignore', 'pipe', 'pipe'] });

  node.stdout.on('data', (d) => process.stdout.write(`[server] ${d}`));
  node.stderr.on('data', (d) => process.stderr.write(`[server] ${d}`));

  // Wait a bit for server to start
  await wait(300);
  return node;
}

async function runTest() {
  const server = await startServer();

  const url = 'ws://localhost:9000';

  const a = new WebSocket(url);
  const b = new WebSocket(url);

  const results = { aPeers: null, bPeers: null, bReceived: null };

  const openPromises = [];
  openPromises.push(new Promise((res) => a.on('open', res)));
  openPromises.push(new Promise((res) => b.on('open', res)));

  await Promise.all(openPromises);

  a.on('message', (data) => {
    const msg = JSON.parse(data.toString());
    if (msg.type === 'peers') {
      results.aPeers = msg.peers;
    }
  });

  b.on('message', (data) => {
    const msg = JSON.parse(data.toString());
    if (msg.type === 'peers') {
      results.bPeers = msg.peers;
    }

    // if it's a direct message, capture it
    if (msg.message) {
      results.bReceived = msg;
    }
  });

  // wait a moment to receive peer lists
  await wait(300);

  // find client ids from peers list
  const aPeers = results.aPeers || [];
  const bPeers = results.bPeers || [];

  console.log('aPeers', aPeers);
  console.log('bPeers', bPeers);

  // pick the peer id for sending
  const targetForA = bPeers.find(id => !aPeers.includes(id)) || bPeers[0] || aPeers[0];

  // send a test message from A to B using server forwarding format (to)
  const testMsg = { type: 'message', to: targetForA, from: 'test-a', message: { hello: 'world' } };
  a.send(JSON.stringify(testMsg));

  // wait for relay
  await wait(300);

  console.log('bReceived', results.bReceived);

  if (!results.bReceived) {
    console.error('Test failed: B did not receive relayed message');
    process.exitCode = 2;
  } else {
    console.log('Test passed: B received message');
  }

  // cleanup
  a.close(); b.close();
  server.kill();
}

runTest().catch(err => { console.error(err); process.exit(1); });
