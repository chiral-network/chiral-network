# 🎯 **QUICK START TESTING GUIDE**

## **Prerequisites (CRITICAL)**

### 1. **Node.js Upgrade Required**
Your current Node.js version (16.20.2) is **too old**. You **MUST** upgrade to Node.js 20.19+ or 22.12+.

**Quick Upgrade Steps:**
```bash
# Option A: Using nvm (recommended)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
source ~/.bashrc  # or restart terminal
nvm install 20
nvm use 20
nvm alias default 20

# Option B: Direct download from https://nodejs.org/
# Download and install Node.js 20.x LTS

# Verify upgrade
node --version  # Should show v20.x.x or higher
```

---

## **Step-by-Step Testing Process**

### **Phase 1: Environment Setup**

1. **Upgrade Node.js** (see above)
2. **Navigate to project directory:**
   ```bash
   cd /Users/jiayiliu/Documents/github416/CSE416/chiral-network
   ```

3. **Install dependencies:**
   ```bash
   npm install
   ```

4. **Verify compilation:**
   ```bash
   # Check TypeScript
   npm run check
   
   # Check Rust
   cd src-tauri && cargo check --quiet && cd ..
   ```

### **Phase 2: Create Test Files**

1. **Run the test file creation script:**
   ```bash
   ./create-test-files.sh
   ```
   This creates:
   - `large-test-file.bin` (2MB) - **triggers multi-source**
   - `medium-test-file.bin` (500KB) - single-source
   - `small-test-file.bin` (100KB) - single-source
   - `test-document.txt` - easy verification

### **Phase 3: Basic Functionality Test**

1. **Start the application:**
   ```bash
   npm run tauri dev
   ```

2. **Test upload functionality:**
   - Go to **Upload** page
   - Upload `test-files/large-test-file.bin`
   - Upload `test-files/medium-test-file.bin`
   - **Note the file hashes** displayed after upload

3. **Test multi-source settings:**
   - Go to **Download** page
   - Verify **"Multi-source"** toggle is enabled
   - Set **"Max peers"** to 3
   - Verify settings persist when you refresh

### **Phase 4: Multi-Source Download Test**

**For a proper multi-source test, you need multiple peers. Here are your options:**

#### **Option A: Single Instance Test (Limited)**
1. Search for your uploaded files using their hashes
2. Download the large file - **should show single-peer mode**
3. Look for these indicators:
   - ✅ Download works normally
   - ❌ Won't show "Multi-source" badge (only 1 peer available)

#### **Option B: Multi-Instance Test (Recommended)**

1. **Keep first instance running** (acts as seeder)

2. **Start second instance in new terminal:**
   ```bash
   # New terminal window
   cd /Users/jiayiliu/Documents/github416/CSE416/chiral-network
   npm run tauri dev -- --port 1421
   ```

3. **From second instance:**
   - Go to Download page
   - Search for large file hash from first instance
   - Download the file
   - **Should trigger multi-source download!**

### **Phase 5: Verify Multi-Source Features**

**✅ Success Indicators:**
- **Purple "Multi-source" badge** appears on large files
- **Peer progress section** shows:
  - "Peers: X/Y" count
  - "Chunks: X/Y" progress
  - Individual peer progress bars
- **Faster download** compared to single-peer
- **Real-time progress updates**

**❌ Expected Limitations:**
- Small files (<1MB) won't use multi-source
- Single-peer files won't show multi-source indicators
- Need actual multiple peers for full testing

---

## **Quick Verification Checklist**

- [ ] ✅ **Node.js 20.19+** installed and verified
- [ ] ✅ **Application builds** without errors (`npm run check` + `cargo check`)
- [ ] ✅ **Application starts** successfully (`npm run tauri dev`)
- [ ] ✅ **File upload** works (can upload test files)
- [ ] ✅ **File download** works (can download uploaded files)
- [ ] ✅ **Multi-source toggle** appears in Download settings
- [ ] ✅ **Large files show** appropriate download behavior
- [ ] ✅ **No crashes** during upload/download operations

---

## **Troubleshooting Common Issues**

### **Issue: "Node.js version too old"**
**Solution:** Upgrade Node.js to 20.19+ (see prerequisites above)

### **Issue: "Failed to load config from vite.config.ts"**
**Solution:** Ensure Node.js is upgraded, then run `npm install`

### **Issue: "Compilation errors"**
**Solution:** Run `npm run check` and `cargo check` to see specific errors

### **Issue: "Multi-source not working"**
**Possible causes:**
- Only one peer available (need multiple instances)
- File too small (<1MB threshold)
- Multi-source disabled in settings

### **Issue: "Downloads fail"**
**Check:**
- File was successfully uploaded and is being seeded
- Network connectivity between instances
- Console logs for specific error messages

---

## **Advanced Testing (Optional)**

### **Performance Comparison**
1. **Single-peer download:**
   - Disable multi-source in settings
   - Download large file, note time

2. **Multi-peer download:**
   - Enable multi-source in settings
   - Download same large file, compare time

### **Error Handling Test**
1. Start multi-source download
2. Stop one instance (simulates peer disconnection)
3. Verify download continues with remaining peers

### **Load Testing**
1. Start multiple downloads simultaneously
2. Verify UI remains responsive
3. Check memory usage doesn't grow excessively

---

## **What to Look For**

### **🎯 Multi-Source Download Success:**
```
✅ Large files (2MB+) show "Multi-source" badge
✅ Progress shows "Peers: 2/3" and "Chunks: 45/128"
✅ Individual peer progress bars visible
✅ Download completes faster than single-peer
✅ Real-time speed and ETA updates
✅ Automatic fallback if peers disconnect
```

### **📊 Expected Console Output:**
```
[INFO] Multi-source download started with 2 peers
[DEBUG] Assigned chunks: peer1 -> 0-63, peer2 -> 64-127
[INFO] Peer connection established: peer_abc123
[DEBUG] Chunk completed: chunk_id=5, peer=peer1
[INFO] Download completed: large-test-file.bin
```

### **🚨 Red Flags:**
```
❌ Application crashes during download
❌ Progress bars freeze or show incorrect data
❌ Memory usage grows continuously
❌ Downloads never complete
❌ UI becomes unresponsive
```

---

## **Next Steps After Testing**

**If tests pass:** 
- Document performance improvements observed
- Note any UI/UX improvements needed
- Consider production deployment

**If tests fail:**
- Document specific failure scenarios
- Check browser and terminal console logs
- Test with different file sizes and peer counts
- Report issues with detailed reproduction steps

---

## **Support**

**Console Logs to Check:**
- **Browser Console (F12):** Frontend errors and progress updates
- **Terminal:** Rust backend logs and compilation errors

**Key Log Patterns:**
- `Multi-source download started` - Feature working
- `Chunk assignment` - Load balancing active  
- `Peer connection established` - P2P working
- `Download completed` - Success!