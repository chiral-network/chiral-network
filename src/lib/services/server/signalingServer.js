const WebSocket = require('ws');

const wss = new WebSocket.Server({ port: 3000 });
const clients = new Map();

wss.on('connection', (ws) => {
  console.log('New client connected');

  ws.on('message', (message) => {
    const data = JSON.parse(message);
    console.log('Received:', data);
    
    if (data.type === 'register') {
      clients.set(data.clientId, ws);
      console.log(`Client registered: ${data.clientId}`);
      broadcastPeers();
      return;
    }

    const targetWs = clients.get(data.to);
    if (targetWs) {
      console.log(`Forwarding message to: ${data.to}`);
      targetWs.send(JSON.stringify(data));
    }
  });

  ws.on('close', () => {
    for (const [clientId, client] of clients.entries()) {
      if (client === ws) {
        console.log(`Client disconnected: ${clientId}`);
        clients.delete(clientId);
        break;
      }
    }
    broadcastPeers();
  });
});

function broadcastPeers() {
  const peerList = Array.from(clients.keys());
  console.log('Current peers:', peerList);
  
  const message = JSON.stringify({
    type: 'peers',
    peers: peerList
  });
  
  for (const client of clients.values()) {
    client.send(message);
  }
}

console.log('Signaling server running on ws://localhost:3000');