# HTTP File Sharing Guide

## Overview

Chiral Network supports HTTP-based P2P file sharing with **automatic internet tunnel support**. Each user runs an HTTP server on their computer, and the system automatically creates a public tunnel (via ngrok, Cloudflare, or other providers) to enable file sharing across the internet without manual port forwarding.

## Key Features

✅ **Automatic Internet Tunneling** - No manual port forwarding needed
✅ **Multiple Tunnel Providers** - Auto-fallback between ngrok, Cloudflare, bore, and localtunnel
✅ **Smart Provider Selection** - Automatically picks the best available provider
✅ **UPnP Support** - Automatic port forwarding for self-hosted mode
✅ **Public IP Detection** - Automatic network configuration
✅ **Real-time Status Monitoring** - Live tunnel connection status

## How It Works

```
Uploader A                                    Internet Tunnel                    Downloader B
    |                                              |                                    |
    | 1. Start HTTP server (Port 8080)             |                                    |
    | 2. Auto-start tunnel (ngrok/cloudflare)      |                                    |
    |    Creates public URL                        |                                    |
    |    (e.g., https://abc123.ngrok.io)           |                                    |
    |                                              |                                    |
    | 3. Upload file                                |                                    |
    | 4. Share public tunnel URL ───────────────►  |  ──────────────────────────────►  |
    |                                              |                                    |
    |                         ◄─────────────── 5. Download via tunnel URL ──────────────|
    |                                              |                                    |
```

## Supported Tunnel Providers

### 1. Ngrok (Most Reliable)
- **Pros**: Industry standard, very reliable, stable connections
- **Cons**: Requires free account and auth token
- **Installation**: `brew install ngrok/ngrok/ngrok` (macOS)
- **Setup**: `ngrok config add-authtoken YOUR_TOKEN`
- **URL Format**: `https://xxxxx.ngrok.io`

### 2. Cloudflare Tunnel (Fast & Free)
- **Pros**: Very fast, no account needed, powered by Cloudflare network
- **Cons**: Random subdomain each time
- **Installation**: `brew install cloudflared` (macOS)
- **Setup**: No configuration needed
- **URL Format**: `https://xxxxx.trycloudflare.com`

### 3. Bore (Lightweight)
- **Pros**: Simple, lightweight, open source
- **Cons**: Less reliable than ngrok/cloudflare
- **Installation**: `cargo install bore-cli`
- **Setup**: No configuration needed
- **URL Format**: `https://xxxxx.bore.pub`

### 4. Localtunnel (Fallback)
- **Pros**: No account needed, works anywhere
- **Cons**: Can be slow, connection issues
- **Installation**: `npm install -g localtunnel`
- **Setup**: No configuration needed
- **URL Format**: `https://xxxxx.loca.lt`

### 5. Self-Hosted (Most Private)
- **Pros**: Maximum privacy, full control
- **Cons**: Requires manual port forwarding or UPnP
- **Setup**: Configure router port forwarding or enable UPnP
- **URL Format**: `http://YOUR_PUBLIC_IP:8080`

## Quick Installation

Run the installation script to install all tunnel providers:

```bash
chmod +x install-tunnel-tools.sh
./install-tunnel-tools.sh
```

This script will automatically install:
- ngrok (via Homebrew)
- cloudflared (via Homebrew)
- bore (via Cargo)
- localtunnel (via npm)

## Usage

### 1. Starting a Tunnel (Frontend)

```javascript
const { invoke } = window.__TAURI__.core;

// Option 1: Auto-select best available provider
const publicUrl = await invoke('start_tunnel_auto', { port: 8080 });
console.log('Public URL:', publicUrl);
// Example: "https://abc123.ngrok.io"

// Option 2: Use specific provider
const publicUrl = await invoke('start_tunnel', {
  port: 8080,
  provider: 'ngrok' // or 'cloudflared', 'bore', 'localtunnel', 'self_hosted'
});

// Check tunnel status
const tunnelInfo = await invoke('get_tunnel_info');
console.log('Tunnel active:', tunnelInfo.is_active);
console.log('Public URL:', tunnelInfo.public_url);
console.log('Provider:', tunnelInfo.provider);
console.log('Status:', tunnelInfo.status); // 'connecting', 'connected', 'failed', 'stopped'
```

### 2. File Upload with Tunnel

```javascript
// Step 1: Start tunnel (if not already running)
const publicUrl = await invoke('start_tunnel_auto', { port: 8080 });

// Step 2: Upload file to local HTTP server
const result = await invoke('upload_file_http', {
  filePath: '/path/to/your/file.pdf'
});

// Step 3: Share the public URL with others
const downloadUrl = `${publicUrl}/download/${result.fileHash}`;
console.log('Share this URL:', downloadUrl);
// Example: https://abc123.ngrok.io/download/a3f2b4c5...
```

### 3. File Download

```javascript
// Download from public tunnel URL
await invoke('download_file_http', {
  fileHash: 'abc123...',
  outputPath: '/downloads/downloaded-file.pdf',
  serverUrl: 'https://abc123.ngrok.io' // Public tunnel URL
});
```

### 4. Stopping the Tunnel

```javascript
await invoke('stop_tunnel');
```

## Tauri Commands

### Tunnel Management

#### `start_tunnel_auto(port: number)`
Automatically selects and starts the best available tunnel provider.

```javascript
const url = await invoke('start_tunnel_auto', { port: 8080 });
// Returns: "https://abc123.ngrok.io"
```

**Auto-selection order:**
1. Ngrok (if installed and configured)
2. Cloudflare (if installed)
3. Bore (if installed)
4. Localtunnel (if installed)
5. Self-hosted (always available as fallback)

#### `start_tunnel(port: number, provider: string)`
Start tunnel with a specific provider.

```javascript
const url = await invoke('start_tunnel', {
  port: 8080,
  provider: 'cloudflared'
});
// Returns: "https://xyz789.trycloudflare.com"
```

**Available providers:**
- `"ngrok"` - Ngrok tunnel
- `"cloudflared"` - Cloudflare tunnel
- `"bore"` - Bore tunnel
- `"localtunnel"` - Localtunnel
- `"self_hosted"` - Self-hosted with UPnP

#### `stop_tunnel()`
Stop the active tunnel.

```javascript
await invoke('stop_tunnel');
```

#### `get_tunnel_info()`
Get current tunnel status and information.

```javascript
const info = await invoke('get_tunnel_info');
// Returns:
// {
//   is_active: true,
//   public_url: "https://abc123.ngrok.io",
//   local_port: 8080,
//   tunnel_type: "ngrok",
//   provider: "ngrok",
//   status: "connected",
//   error_message: null
// }
```

**Status values:**
- `"connecting"` - Tunnel is establishing connection
- `"connected"` - Tunnel is active and ready
- `"failed"` - Tunnel failed to establish
- `"stopped"` - No active tunnel

#### `get_available_providers()`
Get list of available tunnel providers on the system.

```javascript
const providers = await invoke('get_available_providers');
// Returns: ["ngrok", "cloudflared", "bore", "self_hosted"]
```

### Network Information

#### `get_network_info(port: number)`
Gets current network information and automatically sets up UPnP port forwarding (for self-hosted mode).

```javascript
const info = await invoke('get_network_info', { port: 8080 });
// Returns:
// {
//   publicIp: "203.0.113.5",
//   localIp: "192.168.1.100",
//   httpServerUrl: "http://203.0.113.5:8080",
//   upnpEnabled: true,
//   portForwarded: true
// }
```

#### `get_public_ip()`
Gets only the public IP address.

```javascript
const publicIp = await invoke('get_public_ip');
// Returns: "203.0.113.5"
```

#### `get_local_ip()`
Gets the local IP address.

```javascript
const localIp = await invoke('get_local_ip');
// Returns: "192.168.1.100"
```

#### `setup_upnp_port_forwarding(port: number)`
Automatically sets up port forwarding through UPnP.

```javascript
const success = await invoke('setup_upnp_port_forwarding', { port: 8080 });
// Returns: true (success) or false (failure)
```

#### `remove_upnp_port_forwarding(port: number)`
Removes UPnP port forwarding.

```javascript
const success = await invoke('remove_upnp_port_forwarding', { port: 8080 });
```

### File Sharing

#### `upload_file_http(filePath: string, serverUrl?: string)`
Uploads a file to the HTTP server.

```javascript
const result = await invoke('upload_file_http', {
  filePath: '/Users/me/Documents/report.pdf',
  serverUrl: 'http://localhost:8080' // Optional (default)
});
// Returns:
// {
//   fileHash: "a3f2...",
//   fileName: "report.pdf",
//   fileSize: 1024000,
//   uploaderAddress: "self",
//   uploadTime: 1698765432,
//   downloadUrl: "http://localhost:8080/download/a3f2..."
// }
```

#### `download_file_http(fileHash: string, outputPath: string, serverUrl?: string)`
Downloads a file.

```javascript
await invoke('download_file_http', {
  fileHash: 'a3f2...',
  outputPath: '/Users/me/Downloads/report.pdf',
  serverUrl: 'https://abc123.ngrok.io' // Public tunnel URL
});
```

#### `list_files_http(serverUrl?: string)`
Gets the list of all files on the server.

```javascript
const files = await invoke('list_files_http', {
  serverUrl: 'https://abc123.ngrok.io'
});
// Returns: Array of HttpFileInfo
```

#### `get_file_metadata_http(fileHash: string, serverUrl?: string)`
Gets metadata for a specific file.

```javascript
const metadata = await invoke('get_file_metadata_http', {
  fileHash: 'a3f2...',
  serverUrl: 'https://abc123.ngrok.io'
});
```

#### `check_http_server_health(serverUrl?: string)`
Checks the HTTP server status.

```javascript
const isHealthy = await invoke('check_http_server_health', {
  serverUrl: 'http://localhost:8080'
});
// Returns: true (healthy) or false (error)
```

## Complete Workflow Examples

### Scenario 1: Using Auto Tunnel (Recommended)

```javascript
// === Uploader Side ===

// 1. Start tunnel automatically
try {
  const publicUrl = await invoke('start_tunnel_auto', { port: 8080 });
  console.log('✅ Tunnel established:', publicUrl);
} catch (error) {
  console.error('❌ Failed to start tunnel:', error);
  // Fallback to self-hosted if needed
}

// 2. Upload file
const result = await invoke('upload_file_http', {
  filePath: '/path/to/document.pdf'
});

// 3. Get current tunnel info
const tunnelInfo = await invoke('get_tunnel_info');

// 4. Share download URL
const downloadUrl = `${tunnelInfo.public_url}/download/${result.fileHash}`;
console.log('📤 Share this URL:', downloadUrl);
// Example: https://abc123.ngrok.io/download/a3f2b4c5d6e7f8...

// === Downloader Side ===

// Download the file using the shared URL
await invoke('download_file_http', {
  fileHash: 'a3f2b4c5d6e7f8...',
  outputPath: '/downloads/document.pdf',
  serverUrl: 'https://abc123.ngrok.io'
});
```

### Scenario 2: Using Specific Provider (Cloudflare)

```javascript
// === Uploader Side ===

// 1. Check available providers
const providers = await invoke('get_available_providers');
console.log('Available providers:', providers);

// 2. Start Cloudflare tunnel specifically
const publicUrl = await invoke('start_tunnel', {
  port: 8080,
  provider: 'cloudflared'
});
console.log('✅ Cloudflare tunnel:', publicUrl);
// Example: https://xyz789.trycloudflare.com

// 3. Upload and share
const result = await invoke('upload_file_http', {
  filePath: '/path/to/video.mp4'
});

const downloadUrl = `${publicUrl}/download/${result.fileHash}`;
console.log('📤 Share this URL:', downloadUrl);

// === Downloader Side ===
await invoke('download_file_http', {
  fileHash: result.fileHash,
  outputPath: '/downloads/video.mp4',
  serverUrl: publicUrl
});
```

### Scenario 3: Self-Hosted with UPnP

```javascript
// === Uploader Side ===

// 1. Get network info (auto-configures UPnP)
const networkInfo = await invoke('get_network_info', { port: 8080 });
console.log('Public IP:', networkInfo.publicIp);
console.log('UPnP enabled:', networkInfo.upnpEnabled);

// 2. Start self-hosted mode
const localUrl = await invoke('start_tunnel', {
  port: 8080,
  provider: 'self_hosted'
});

// 3. Upload file
const result = await invoke('upload_file_http', {
  filePath: '/path/to/file.zip'
});

// 4. Share public URL (if UPnP succeeded)
if (networkInfo.upnpEnabled) {
  const downloadUrl = `http://${networkInfo.publicIp}:8080/download/${result.fileHash}`;
  console.log('📤 Share this URL:', downloadUrl);
} else {
  console.warn('⚠️ UPnP failed. Manual port forwarding required.');
}
```

### Scenario 4: Monitoring Tunnel Status

```javascript
// Start tunnel with status monitoring
const publicUrl = await invoke('start_tunnel_auto', { port: 8080 });

// Poll tunnel status
const checkStatus = setInterval(async () => {
  const info = await invoke('get_tunnel_info');

  console.log('Tunnel status:', info.status);

  if (info.status === 'connected') {
    console.log('✅ Tunnel is active:', info.public_url);
    console.log('Provider:', info.provider);
  } else if (info.status === 'failed') {
    console.error('❌ Tunnel failed:', info.error_message);
    clearInterval(checkStatus);
  } else if (info.status === 'connecting') {
    console.log('⏳ Connecting...');
  }
}, 1000);

// Stop monitoring after 30 seconds
setTimeout(() => clearInterval(checkStatus), 30000);
```

## UI Component: TunnelManager

Chiral Network includes a built-in Svelte component for tunnel management:

```svelte
<script>
  import TunnelManager from '$lib/components/TunnelManager.svelte';
</script>

<TunnelManager />
```

**Features:**
- Auto-detect available providers
- Start/stop tunnel with one click
- Real-time status monitoring
- Provider selection dropdown
- Public URL display with copy button
- Error message display
- Provider information and recommendations

## Provider Setup Guide

### Ngrok Setup

1. **Install ngrok:**
   ```bash
   brew install ngrok/ngrok/ngrok
   ```

2. **Create account:** Visit [ngrok.com](https://ngrok.com) and sign up

3. **Get auth token:** Copy from dashboard

4. **Configure:**
   ```bash
   ngrok config add-authtoken YOUR_TOKEN_HERE
   ```

5. **Test:**
   ```bash
   ngrok http 8080
   ```

### Cloudflare Setup

1. **Install cloudflared:**
   ```bash
   brew install cloudflared
   ```

2. **Test:**
   ```bash
   cloudflared tunnel --url http://localhost:8080
   ```

**No account or configuration needed!**

### Bore Setup

1. **Install Rust** (if not installed):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Install bore:**
   ```bash
   cargo install bore-cli
   ```

3. **Test:**
   ```bash
   bore local 8080 --to bore.pub
   ```

### Localtunnel Setup

1. **Install Node.js** (if not installed)

2. **Install localtunnel:**
   ```bash
   npm install -g localtunnel
   ```

3. **Test:**
   ```bash
   lt --port 8080
   ```

## Security Considerations

### Current Implementation

⚠️ **Tunnel Security:**
- Most tunnels use HTTPS (encrypted in transit)
- ngrok: HTTPS with TLS 1.2+
- Cloudflare: HTTPS with TLS 1.3
- bore: HTTPS
- localtunnel: HTTPS
- self-hosted: HTTP (unencrypted)

⚠️ **Access Control:**
- No authentication by default
- Anyone with the tunnel URL can access files
- URLs are random and hard to guess (security through obscurity)

⚠️ **Data Privacy:**
- Tunnel providers can see your traffic
- ngrok/Cloudflare/bore route through their servers
- Self-hosted is most private (direct connection)

### Best Practices

1. **Use Temporary Tunnels:**
   - Start tunnel only when sharing
   - Stop tunnel after download completes
   - Don't leave tunnels running indefinitely

2. **Share URLs Privately:**
   - Send URLs via secure channels (Signal, encrypted email)
   - Don't post tunnel URLs publicly
   - Use password-protected communication

3. **Monitor Active Tunnels:**
   - Check tunnel status regularly
   - Stop unused tunnels
   - Review file access logs

4. **For Maximum Privacy:**
   - Use self-hosted mode with manual port forwarding
   - Or use Cloudflare (better privacy than ngrok)
   - Avoid leaving tunnels open overnight

### Future Security Enhancements

- Password-protected tunnel access
- Token-based authentication
- IP whitelist/blacklist
- Download count limits
- File expiration
- End-to-end encryption (separate from tunnel encryption)

## Troubleshooting

### 1. Auto Tunnel Fails

**Symptom:** `start_tunnel_auto` returns error "No tunnel providers available"

**Solution:**
```bash
# Check which providers are installed
ngrok version          # Should show version number
cloudflared version    # Should show version number
bore --help           # Should show help text
lt --version          # Should show version number

# Install missing providers
./install-tunnel-tools.sh
```

### 2. Ngrok Authentication Failed

**Symptom:** `status: "failed"`, error mentions "authentication" or "authtoken"

**Solution:**
```bash
# Configure ngrok with your auth token
ngrok config add-authtoken YOUR_TOKEN

# Verify configuration
cat ~/.ngrok2/ngrok.yml
```

### 3. Tunnel Connects But No URL

**Symptom:** `status: "connecting"` stays indefinitely

**Solution:**
- Check firewall settings (allow outbound connections)
- Test tunnel manually: `ngrok http 8080`
- Try different provider: `start_tunnel({ port: 8080, provider: 'cloudflared' })`
- Check network connectivity

### 4. Cloudflare Tunnel Slow

**Symptom:** Tunnel connects but transfers are slow

**Solution:**
- Cloudflare tunnels can be slower than ngrok for large files
- Try ngrok or bore instead
- For best performance with large files, use self-hosted mode

### 5. Self-Hosted Mode Not Accessible

**Symptom:** Self-hosted tunnel starts but external access fails

**Solution:**
```javascript
// Check UPnP status
const info = await invoke('get_network_info', { port: 8080 });
if (!info.upnpEnabled) {
  console.log('UPnP failed. Manual port forwarding needed.');
  // Instructions:
  // 1. Access router admin page (usually 192.168.1.1)
  // 2. Find "Port Forwarding" settings
  // 3. Forward external port 8080 to internal IP and port 8080
}
```

### 6. "Address Already in Use" Error

**Symptom:** HTTP server fails to start on port 8080

**Solution:**
```bash
# Check what's using port 8080
lsof -i :8080

# Kill the process
kill -9 <PID>

# Or use different port
await invoke('start_tunnel_auto', { port: 8081 });
```

### 7. Tunnel Disconnects Randomly

**Symptom:** Tunnel URL becomes unavailable during file transfer

**Solution:**
- ngrok free tier has session limits (2 hours)
- Use Cloudflare or bore for longer sessions
- Implement auto-reconnect logic:
  ```javascript
  const reconnectTunnel = async () => {
    const info = await invoke('get_tunnel_info');
    if (info.status === 'failed') {
      console.log('Reconnecting tunnel...');
      await invoke('stop_tunnel');
      await invoke('start_tunnel_auto', { port: 8080 });
    }
  };

  setInterval(reconnectTunnel, 30000); // Check every 30s
  ```

### 8. Tunnel Provider Not Found

**Symptom:** Specific provider fails with "not installed" error

**Solution:**
```bash
# Verify installation
which ngrok         # Should show path
which cloudflared   # Should show path
which bore         # Should show path
which lt           # Should show path

# If missing, install via script
./install-tunnel-tools.sh

# Or install individually
brew install ngrok/ngrok/ngrok        # ngrok
brew install cloudflared              # cloudflare
cargo install bore-cli                # bore
npm install -g localtunnel           # localtunnel
```

## Limitations

1. **Uploader Must Keep App Running**
   - Downloader can only access files while uploader's app is running
   - Tunnel stays active only while app is open

2. **Free Tier Limits**
   - Ngrok: 1 tunnel, 40 connections/min, 2 hour sessions
   - Cloudflare: No limits (but random URLs)
   - Bore: Community server (shared resources)
   - Localtunnel: Can be unstable

3. **Bandwidth**
   - Limited by uploader's internet connection
   - Tunnel providers may throttle bandwidth
   - Multiple simultaneous downloads will slow down

4. **Tunnel Provider Availability**
   - External services can go down
   - Rate limiting during high usage
   - Geographic restrictions may apply

5. **Dynamic URLs**
   - Cloudflare/bore/localtunnel generate random URLs each time
   - Ngrok can use custom subdomains (paid tier)
   - URLs change when tunnel restarts

## Advanced Configuration

### Custom Ngrok Configuration

Create `~/.ngrok2/ngrok.yml`:
```yaml
version: "2"
authtoken: YOUR_AUTH_TOKEN
tunnels:
  chiral:
    proto: http
    addr: 8080
    subdomain: my-chiral-network  # Requires paid plan
    bind_tls: true
```

### Cloudflare with Custom Domain

```bash
# Set up Cloudflare Tunnel with custom domain
cloudflared tunnel login
cloudflared tunnel create chiral-network
cloudflared tunnel route dns chiral-network share.yourdomain.com
```

### Multiple Simultaneous Tunnels

```javascript
// Start tunnels on different ports
const tunnel1 = await invoke('start_tunnel', { port: 8080, provider: 'ngrok' });
const tunnel2 = await invoke('start_tunnel', { port: 8081, provider: 'cloudflared' });

// Use different tunnels for different files
console.log('Video files:', tunnel1);
console.log('Document files:', tunnel2);
```

## Summary

✅ **Implemented:**
- HTTP file server (Port 8080)
- Automatic internet tunneling
- Multiple tunnel providers (ngrok, Cloudflare, bore, localtunnel)
- Smart provider auto-selection
- Real-time tunnel status monitoring
- UPnP port forwarding support
- Public IP detection
- File upload/download via tunnel URLs
- Tunnel management UI component

✅ **Use Cases:**
- Person-to-person file sharing across the internet
- No manual port forwarding needed
- Quick file sharing with auto-generated URLs
- Local network file sharing (self-hosted mode)
- Educational and personal file distribution

⚠️ **Important Notes:**
- Uploader must keep app running during file transfer
- Free tunnel providers have limitations
- URLs are temporary and change on restart (except ngrok paid)
- For maximum privacy, use self-hosted mode
- Tunnel providers can see your traffic (except self-hosted)

## Getting Help

If you encounter issues:

1. **Check tunnel status:**
   ```javascript
   const info = await invoke('get_tunnel_info');
   console.log(info);
   ```

2. **Check available providers:**
   ```javascript
   const providers = await invoke('get_available_providers');
   console.log('Available:', providers);
   ```

3. **Test manually:**
   ```bash
   ngrok http 8080
   cloudflared tunnel --url http://localhost:8080
   bore local 8080 --to bore.pub
   lt --port 8080
   ```

4. **Check logs:** Look for tunnel output in application logs

5. **Try different provider:** If one fails, try another

6. **Reinstall tools:** Run `./install-tunnel-tools.sh` again

For more help, consult the main project documentation or open an issue on GitHub.
