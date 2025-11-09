// WebSocket signaling server for peer discovery and WebRTC signaling
const WebSocket = require("ws");
const port = parseInt(process.env.PORT || "9000", 10);
// Bind explicitly to IPv4 loopback to avoid IPv6/::1 resolution issues in tests
const host = process.env.HOST || "127.0.0.1";
const wss = new WebSocket.Server({ port, host });

// Emit a clear listening message only after the underlying server is actually
// listening. This prevents tests from racing on a premature stdout log.
try {
  const underlying = wss._server;
  if (underlying && typeof underlying.on === "function") {
    underlying.on("listening", () => {
      console.log(`[SignalingServer] listening on ws://${host}:${port}`);
    });
    underlying.on("error", (err) => {
      console.error("[SignalingServer] underlying server error:", err);
    });
  } else {
    // Fallback: if no underlying server, print immediately
    console.log(`[SignalingServer] listening on ws://${host}:${port}`);
  }
} catch (e) {
  console.error("[SignalingServer] failed to attach listening handler", e);
  console.log(`[SignalingServer] listening on ws://${host}:${port}`);
}

// Graceful shutdown helpers for test harnesses
process.on("SIGTERM", () => {
  try {
    wss.close(() => process.exit(0));
  } catch (e) {
    process.exit(0);
  }
});
process.on("SIGINT", () => {
  try {
    wss.close(() => process.exit(0));
  } catch (e) {
    process.exit(0);
  }
});

// Track connected peers
const peers = new Map(); // clientId -> WebSocket

wss.on("connection", function connection(ws) {
  ws.on("message", function incoming(data) {
    try {
      const message = JSON.parse(data);

      if (message.type === "register") {
        // Register new peer
        peers.set(message.clientId, ws);
        ws.clientId = message.clientId;

        // Send current peer list to new client
        const peerList = Array.from(peers.keys()).filter(
          (id) => id !== message.clientId
        );
        ws.send(
          JSON.stringify({
            type: "peers",
            peers: peerList,
          })
        );

        // Notify other peers about new peer
        broadcastToPeers(
          {
            type: "peers",
            peers: Array.from(peers.keys()),
          },
          message.clientId
        );
      } else if (message.to) {
        // Route message to specific peer
        const targetWs = peers.get(message.to);
        if (targetWs && targetWs.readyState === WebSocket.OPEN) {
          targetWs.send(JSON.stringify(message));
        }
      } else {
        // Broadcast to all other clients (fallback)
        wss.clients.forEach(function each(client) {
          if (client !== ws && client.readyState === WebSocket.OPEN) {
            client.send(JSON.stringify(message));
          }
        });
      }
    } catch (error) {
      console.error("Error processing message:", error);
    }
  });

  ws.on("close", function () {
    if (ws.clientId) {
      peers.delete(ws.clientId);

      // Notify remaining peers about disconnection
      broadcastToPeers({
        type: "peers",
        peers: Array.from(peers.keys()),
      });
    }
  });

  ws.on("error", function (error) {
    console.error("WebSocket error:", error);
  });
});

function broadcastToPeers(message, excludeClientId = null) {
  peers.forEach((ws, clientId) => {
    if (clientId !== excludeClientId && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(message));
    }
  });
}
