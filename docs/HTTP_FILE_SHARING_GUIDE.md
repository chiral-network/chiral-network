# HTTP File Sharing Guide

## Overview

Chiral Network supports HTTP-based P2P file sharing. Each user runs an HTTP server on their computer and can share files with other users through their public IP.

## How It Works

```
Uploader A                                  Downloader B
    |                                            |
    | 1. Start HTTP server (Port 8080)           |
    | 2. Automatic UPnP port forwarding          |
    | 3. Check public IP                         |
    |    (e.g., 203.0.113.5)                     |
    | 4. Upload file                             |
    |                                            |
    | 5. Share URL ─────────────────────────────►|
    |    http://203.0.113.5:8080                 |
    |                                            |
    |◄──────────── 6. Download file ─────────────|
```

## How to Use

### 1. File Upload (Sharer)

```javascript
const { invoke } = window.__TAURI__.core;

// Step 1: Get network information (public IP + UPnP setup)
const networkInfo = await invoke('get_network_info', { port: 8080 });
console.log('Your sharing URL:', networkInfo.httpServerUrl);
console.log('UPnP enabled:', networkInfo.upnpEnabled);

// Step 2: Upload file
const result = await invoke('upload_file_http', {
  filePath: '/path/to/your/file.pdf'
});
console.log('File hash:', result.fileHash);
console.log('Download URL:', result.downloadUrl);

// Step 3: Share URL with other users
// URL: http://<your-public-ip>:8080/download/<file-hash>
```

### 2. File Download (Downloader)

```javascript
// Step 1: Use the shared server URL
const uploaderUrl = 'http://203.0.113.5:8080'; // Uploader's public IP

// Step 2: Check file list (optional)
const files = await invoke('list_files_http', {
  serverUrl: uploaderUrl
});
console.log('Available files:', files);

// Step 3: Download file
await invoke('download_file_http', {
  fileHash: 'abc123...',
  outputPath: '/downloads/downloaded-file.pdf',
  serverUrl: uploaderUrl
});
```

## Tauri Commands

### Network Information

#### `get_network_info(port: number)`
Gets current network information and automatically sets up UPnP port forwarding.

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
  serverUrl: 'http://203.0.113.5:8080' // Uploader's URL
});
```

#### `list_files_http(serverUrl?: string)`
Gets the list of all files on the server.

```javascript
const files = await invoke('list_files_http', {
  serverUrl: 'http://203.0.113.5:8080'
});
// Returns: Array of HttpFileInfo
```

#### `get_file_metadata_http(fileHash: string, serverUrl?: string)`
Gets metadata for a specific file.

```javascript
const metadata = await invoke('get_file_metadata_http', {
  fileHash: 'a3f2...',
  serverUrl: 'http://203.0.113.5:8080'
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

## Port Forwarding

### Automatic (UPnP)
The app automatically forwards port 8080 using the router's UPnP feature.

**Advantages:**
- No user configuration required
- Handled automatically

**Disadvantages:**
- Not all routers support UPnP
- Some routers have UPnP disabled for security reasons

### Manual
If UPnP doesn't work, you need to manually set up port forwarding in your router settings.

**Setup Method:**
1. Access router management page (e.g., http://192.168.1.1)
2. Find port forwarding settings
3. Add new rule:
   - External port: 8080
   - Internal IP: (local IP, e.g., 192.168.1.100)
   - Internal port: 8080
   - Protocol: TCP

## Security Considerations

### Current Implementation
- ⚠️ HTTP (unencrypted)
- ⚠️ No authentication
- ⚠️ Anyone with the IP can access

### Future Improvements
- HTTPS support (Let's Encrypt)
- Token-based authentication
- IP whitelist
- Download count limits

## Troubleshooting

### 1. UPnP Port Forwarding Failure
**Symptom:** `upnpEnabled: false`

**Solution:**
- Enable UPnP on router
- Set up port forwarding manually
- Check firewall

### 2. Cannot Get Public IP
**Symptom:** `get_public_ip` command fails

**Solution:**
- Check internet connection
- Check if firewall blocks external API access

### 3. Download Failure
**Symptom:** Error when downloading file

**Solution:**
- Verify uploader's app is running
- Verify uploader's IP/URL is correct
- Check firewall/port forwarding settings

### 4. "Address already in use" Error
**Symptom:** HTTP server fails to start (Port 8080)

**Solution:**
```bash
# Check process using the port
lsof -i :8080

# Kill the process
kill -9 <PID>
```

## Example Scenarios

### Scenario 1: Same Network (Local)
```javascript
// Uploader
const result = await invoke('upload_file_http', {
  filePath: '/path/to/file.pdf'
});

// Downloader (same network)
await invoke('download_file_http', {
  fileHash: result.fileHash,
  outputPath: '/downloads/file.pdf',
  serverUrl: 'http://192.168.1.100:8080' // Use local IP
});
```

### Scenario 2: Internet (Different Network)
```javascript
// Uploader
const networkInfo = await invoke('get_network_info', { port: 8080 });
console.log('Share this URL:', networkInfo.httpServerUrl);

const result = await invoke('upload_file_http', {
  filePath: '/path/to/file.pdf'
});

// Share URL: http://203.0.113.5:8080/download/<hash>

// Downloader (different network)
await invoke('download_file_http', {
  fileHash: '<hash>',
  outputPath: '/downloads/file.pdf',
  serverUrl: 'http://203.0.113.5:8080' // Use public IP
});
```

## Limitations

1. **Uploader Must Keep App Running**
   - The uploader's app must be running for downloaders to receive files

2. **Dynamic IP**
   - URL changes if public IP changes
   - DDNS service recommended (e.g., No-IP, DuckDNS)

3. **Bandwidth**
   - Limited by uploader's upload speed
   - May slow down if multiple users download simultaneously

4. **Firewall/NAT**
   - Port forwarding may not be possible in some networks (corporate, school, mobile hotspot, etc.)

## Summary

✅ **Implemented:**
- HTTP file server (Port 8080)
- File upload/download
- Automatic public IP detection
- Automatic UPnP port forwarding
- File metadata management

✅ **Use Cases:**
- Person-to-person file sharing (internet)
- Local network file sharing
- Simple and fast setup

⚠️ **Cautions:**
- Uploader must keep app running
- HTTP (unencrypted)
- Port forwarding required (automatic or manual)
