# HTTP 파일 공유 가이드

## 개요

Chiral Network는 HTTP 기반 P2P 파일 공유를 지원합니다. 각 사용자가 자신의 컴퓨터에서 HTTP 서버를 실행하고, 공인 IP를 통해 다른 사용자와 파일을 공유할 수 있습니다.

## 작동 방식

```
업로더 A                                    다운로더 B
    |                                            |
    | 1. HTTP 서버 시작 (Port 8080)              |
    | 2. UPnP 자동 포트 포워딩                    |
    | 3. 공인 IP 확인                             |
    |    (예: 203.0.113.5)                       |
    | 4. 파일 업로드                              |
    |                                            |
    | 5. URL 공유 ─────────────────────────────►|
    |    http://203.0.113.5:8080                 |
    |                                            |
    |◄──────────── 6. 파일 다운로드 ─────────────|
```

## 사용 방법

### 1. 파일 업로드 (공유자)

```javascript
const { invoke } = window.__TAURI__.core;

// Step 1: 네트워크 정보 가져오기 (공인 IP + UPnP 설정)
const networkInfo = await invoke('get_network_info', { port: 8080 });
console.log('Your sharing URL:', networkInfo.httpServerUrl);
console.log('UPnP enabled:', networkInfo.upnpEnabled);

// Step 2: 파일 업로드
const result = await invoke('upload_file_http', {
  filePath: '/path/to/your/file.pdf'
});
console.log('File hash:', result.fileHash);
console.log('Download URL:', result.downloadUrl);

// Step 3: 다른 사용자에게 URL 공유
// URL: http://<your-public-ip>:8080/download/<file-hash>
```

### 2. 파일 다운로드 (다운로더)

```javascript
// Step 1: 공유받은 서버 URL 사용
const uploaderUrl = 'http://203.0.113.5:8080'; // 업로더의 공인 IP

// Step 2: 파일 목록 확인 (선택사항)
const files = await invoke('list_files_http', {
  serverUrl: uploaderUrl
});
console.log('Available files:', files);

// Step 3: 파일 다운로드
await invoke('download_file_http', {
  fileHash: 'abc123...',
  outputPath: '/downloads/downloaded-file.pdf',
  serverUrl: uploaderUrl
});
```

## Tauri 명령어

### 네트워크 정보

#### `get_network_info(port: number)`
현재 네트워크 정보를 가져오고 UPnP 포트 포워딩을 자동 설정합니다.

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
공인 IP 주소만 가져옵니다.

```javascript
const publicIp = await invoke('get_public_ip');
// Returns: "203.0.113.5"
```

#### `get_local_ip()`
로컬 IP 주소를 가져옵니다.

```javascript
const localIp = await invoke('get_local_ip');
// Returns: "192.168.1.100"
```

#### `setup_upnp_port_forwarding(port: number)`
UPnP를 통해 자동으로 포트 포워딩을 설정합니다.

```javascript
const success = await invoke('setup_upnp_port_forwarding', { port: 8080 });
// Returns: true (성공) 또는 false (실패)
```

#### `remove_upnp_port_forwarding(port: number)`
UPnP 포트 포워딩을 제거합니다.

```javascript
const success = await invoke('remove_upnp_port_forwarding', { port: 8080 });
```

### 파일 공유

#### `upload_file_http(filePath: string, serverUrl?: string)`
파일을 HTTP 서버에 업로드합니다.

```javascript
const result = await invoke('upload_file_http', {
  filePath: '/Users/me/Documents/report.pdf',
  serverUrl: 'http://localhost:8080' // 선택사항 (기본값)
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
파일을 다운로드합니다.

```javascript
await invoke('download_file_http', {
  fileHash: 'a3f2...',
  outputPath: '/Users/me/Downloads/report.pdf',
  serverUrl: 'http://203.0.113.5:8080' // 업로더의 URL
});
```

#### `list_files_http(serverUrl?: string)`
서버의 모든 파일 목록을 가져옵니다.

```javascript
const files = await invoke('list_files_http', {
  serverUrl: 'http://203.0.113.5:8080'
});
// Returns: Array of HttpFileInfo
```

#### `get_file_metadata_http(fileHash: string, serverUrl?: string)`
특정 파일의 메타데이터를 가져옵니다.

```javascript
const metadata = await invoke('get_file_metadata_http', {
  fileHash: 'a3f2...',
  serverUrl: 'http://203.0.113.5:8080'
});
```

#### `check_http_server_health(serverUrl?: string)`
HTTP 서버 상태를 확인합니다.

```javascript
const isHealthy = await invoke('check_http_server_health', {
  serverUrl: 'http://localhost:8080'
});
// Returns: true (정상) 또는 false (오류)
```

## 포트 포워딩

### 자동 (UPnP)
앱이 자동으로 라우터의 UPnP 기능을 사용하여 포트 8080을 포워딩합니다.

**장점:**
- 사용자 설정 불필요
- 자동으로 처리됨

**단점:**
- 모든 라우터가 UPnP를 지원하는 것은 아님
- 일부 라우터는 보안상 UPnP가 비활성화되어 있음

### 수동
UPnP가 작동하지 않는 경우, 라우터 설정에서 수동으로 포트 포워딩을 설정해야 합니다.

**설정 방법:**
1. 라우터 관리 페이지 접속 (예: http://192.168.1.1)
2. 포트 포워딩 설정 찾기
3. 새 규칙 추가:
   - 외부 포트: 8080
   - 내부 IP: (로컬 IP, 예: 192.168.1.100)
   - 내부 포트: 8080
   - 프로토콜: TCP

## 보안 고려사항

### 현재 구현
- ⚠️ HTTP (암호화되지 않음)
- ⚠️ 인증 없음
- ⚠️ 누구나 IP를 알면 접근 가능

### 개선 사항 (향후)
- HTTPS 지원 (Let's Encrypt)
- 토큰 기반 인증
- IP 화이트리스트
- 다운로드 횟수 제한

## 문제 해결

### 1. UPnP 포트 포워딩 실패
**증상:** `upnpEnabled: false`

**해결:**
- 라우터에서 UPnP 활성화
- 수동으로 포트 포워딩 설정
- 방화벽 확인

### 2. 공인 IP를 가져올 수 없음
**증상:** `get_public_ip` 명령 실패

**해결:**
- 인터넷 연결 확인
- 방화벽이 외부 API 접근 차단 여부 확인

### 3. 다운로드 실패
**증상:** 파일 다운로드 시 오류

**해결:**
- 업로더의 앱이 실행 중인지 확인
- 업로더의 IP/URL이 정확한지 확인
- 방화벽/포트 포워딩 설정 확인

### 4. "Address already in use" 오류
**증상:** HTTP 서버 시작 실패 (Port 8080)

**해결:**
```bash
# 포트 사용 중인 프로세스 확인
lsof -i :8080

# 프로세스 종료
kill -9 <PID>
```

## 예제 시나리오

### 시나리오 1: 같은 네트워크 (로컬)
```javascript
// 업로더
const result = await invoke('upload_file_http', {
  filePath: '/path/to/file.pdf'
});

// 다운로더 (같은 네트워크)
await invoke('download_file_http', {
  fileHash: result.fileHash,
  outputPath: '/downloads/file.pdf',
  serverUrl: 'http://192.168.1.100:8080' // 로컬 IP 사용
});
```

### 시나리오 2: 인터넷 (다른 네트워크)
```javascript
// 업로더
const networkInfo = await invoke('get_network_info', { port: 8080 });
console.log('Share this URL:', networkInfo.httpServerUrl);

const result = await invoke('upload_file_http', {
  filePath: '/path/to/file.pdf'
});

// URL 공유: http://203.0.113.5:8080/download/<hash>

// 다운로더 (다른 네트워크)
await invoke('download_file_http', {
  fileHash: '<hash>',
  outputPath: '/downloads/file.pdf',
  serverUrl: 'http://203.0.113.5:8080' // 공인 IP 사용
});
```

## 제한사항

1. **업로더가 앱을 실행해야 함**
   - 다운로더가 파일을 받으려면 업로더의 앱이 켜져 있어야 함

2. **동적 IP**
   - 공인 IP가 변경되면 URL도 변경됨
   - DDNS 서비스 사용 권장 (예: No-IP, DuckDNS)

3. **대역폭**
   - 업로더의 업로드 속도에 제한됨
   - 여러 사용자가 동시에 다운로드하면 느려질 수 있음

4. **방화벽/NAT**
   - 일부 네트워크에서는 포트 포워딩이 불가능할 수 있음 (회사, 학교, 모바일 핫스팟 등)

## 요약

✅ **구현됨:**
- HTTP 파일 서버 (Port 8080)
- 파일 업로드/다운로드
- 공인 IP 자동 감지
- UPnP 자동 포트 포워딩
- 파일 메타데이터 관리

✅ **사용 가능:**
- 개인 간 파일 공유 (인터넷)
- 로컬 네트워크 파일 공유
- 간단하고 빠른 설정

⚠️ **주의사항:**
- 업로더가 앱을 계속 실행해야 함
- HTTP (암호화되지 않음)
- 포트 포워딩 필요 (자동 또는 수동)
