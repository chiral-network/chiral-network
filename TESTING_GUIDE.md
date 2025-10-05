# 🧪 Multi-Source Download Testing Guide

## Prerequisites
1. **Upgrade Node.js to 20.19+ or 22.12+**
2. **Ensure Rust and Cargo are installed**
3. **Have multiple network peers available for testing**

## Step-by-Step Testing Process

### Phase 1: Environment Preparation

1. **Upgrade Node.js** (if not done already):
   ```bash
   # Using nvm (recommended)
   nvm install 20
   nvm use 20
   nvm alias default 20
   
   # Verify version
   node --version  # Should show v20.x.x or higher
   ```

2. **Run the setup script**:
   ```bash
   ./test-multi-source.sh
   ```

3. **Create test files**:
   ```bash
   ./create-test-files.sh
   ```

### Phase 2: Application Testing

#### Test 1: Single Instance Testing (Basic Functionality)

1. **Start the application**:
   ```bash
   npm run tauri dev
   ```

2. **Upload test files to network**:
   - Go to Upload page
   - Upload `test-files/large-test-file.bin` (2MB)
   - Upload `test-files/medium-test-file.bin` (500KB)
   - Upload `test-files/test-document.txt`
   - Note the file hashes for each upload

3. **Test multi-source download settings**:
   - Go to Download page
   - Check the "Multi-source" toggle is enabled
   - Set "Max peers" to 3
   - Verify settings are saved

#### Test 2: Multi-Instance Testing (Real Multi-Source)

1. **Start first instance** (Terminal 1):
   ```bash
   npm run tauri dev
   ```

2. **Start second instance** (Terminal 2):
   ```bash
   # In a new terminal window
   cd /path/to/chiral-network
   npm run tauri dev -- --port 1421
   ```

3. **Upload files from Instance 1**:
   - Upload all test files
   - Keep Instance 1 running as a seeder

4. **Download from Instance 2**:
   - Search for uploaded files using their hashes
   - Download the large file (should trigger multi-source)
   - Monitor progress and peer information

### Phase 3: Feature Verification

#### Test 3: Multi-Source Download Behavior

**Expected Behaviors:**

1. **Large files (>1MB with 2+ seeders)**:
   - ✅ Should show "Multi-source" badge
   - ✅ Should display peer progress bars
   - ✅ Should show "Peers: X/Y" in progress
   - ✅ Should show "Chunks: X/Y" information

2. **Small files (<1MB or single seeder)**:
   - ✅ Should use single-peer download
   - ✅ Should not show multi-source indicators

3. **Progress indicators**:
   - ✅ Overall progress bar updates smoothly
   - ✅ Individual peer progress bars show activity
   - ✅ Speed and ETA calculations are reasonable

#### Test 4: Error Handling

1. **Test peer disconnection**:
   - Start multi-source download
   - Stop one seeder instance
   - Verify download continues with remaining peers

2. **Test download cancellation**:
   - Start large file download
   - Cancel during progress
   - Verify cleanup and status updates

3. **Test retry functionality**:
   - Force a download failure
   - Use retry button
   - Verify new download attempt

### Phase 4: Performance Verification

#### Test 5: Speed Comparison

1. **Single-peer download**:
   - Disable multi-source
   - Download large file
   - Note completion time

2. **Multi-peer download**:
   - Enable multi-source
   - Download same large file
   - Compare completion time

#### Test 6: UI Responsiveness

1. **Multiple concurrent downloads**:
   - Start 3-4 downloads simultaneously
   - Verify UI remains responsive
   - Check progress updates for all downloads

2. **Large file handling**:
   - Create a 10MB+ test file
   - Verify chunk management
   - Monitor memory usage

## Expected Results

### ✅ Success Indicators

- **Multi-source detection**: Large files automatically use multi-source
- **Visual feedback**: Purple "Multi-source" badges appear
- **Progress tracking**: Individual peer progress bars display
- **Performance**: Multi-source downloads complete faster than single-source
- **Error recovery**: Downloads continue when peers disconnect
- **Settings persistence**: Multi-source preferences are saved

### ❌ Failure Indicators

- Application crashes during download
- Progress bars freeze or show incorrect data
- Downloads fail to start or complete
- Memory leaks during large file transfers
- UI becomes unresponsive

## Debugging

### Console Logs to Monitor

1. **Browser Console** (F12):
   ```
   Multi-source download started with X peers
   Chunk assignment: peer1 -> chunks 0-5, peer2 -> chunks 6-10
   Progress update: 45% (peer1: 60%, peer2: 30%)
   ```

2. **Rust Logs** (Terminal):
   ```
   [INFO] MultiSourceDownloadService: Starting download for file_hash
   [INFO] Assigned chunks to 3 peers
   [DEBUG] Peer connection established: peer_id_123
   ```

### Common Issues and Solutions

1. **"No peers available"**:
   - Ensure multiple instances are running
   - Check network connectivity
   - Verify file was uploaded and is being seeded

2. **Single-peer fallback**:
   - File might be too small (<1MB)
   - Only one seeder available
   - Multi-source setting disabled

3. **Slow performance**:
   - Check system resources
   - Verify network connection
   - Consider reducing max peers setting

## Test Completion Checklist

- [ ] Environment setup completed successfully
- [ ] Application builds and runs without errors
- [ ] Multi-source downloads work with multiple peers
- [ ] Progress indicators display correctly
- [ ] Error handling works properly
- [ ] Performance improvements are measurable
- [ ] UI remains responsive during downloads
- [ ] Settings are saved and applied correctly

## Next Steps

If all tests pass:
1. **Document any performance improvements**
2. **Note any edge cases discovered**
3. **Consider additional optimizations**
4. **Prepare for production deployment**

If tests fail:
1. **Document specific failure scenarios**
2. **Check console logs for error details**
3. **Review code for potential issues**
4. **Test with different file sizes and peer counts**