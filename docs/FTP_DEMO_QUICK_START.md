# FTP Demo - Quick Start Guide

## For Class Presentation

### **How to Run the Demo**

**Step 1: Check prerequisites**
```powershell
# Windows
.\src-tauri\examples\check_ftp_setup.ps1

# Mac/Linux
bash src-tauri/examples/check_ftp_setup.sh
```

**Step 2: Create directory and test file**
```bash
# Windows
mkdir C:\FTP_Test
echo This is a test file > C:\FTP_Test\test_file.txt

# Mac/Linux
mkdir -p /tmp/ftp_test
echo "This is a test file" > /tmp/ftp_test/test_file.txt
```

**Step 3: Start local FTP server (Terminal 1)**
```bash
# Windows
python -m pyftpdlib -p 21 -w -d C:\FTP_Test

# Mac/Linux
python3 -m pyftpdlib -p 21 -w -d /tmp/ftp_test
```

**Step 4: Run the demo (Terminal 2)**
```bash
cd src-tauri
cargo run --example ftp_demo local
```

✅ **Result**: Shows real file download from local FTP server

---

## What This Demonstrates

- ✅ FTP client works
- ✅ Cross-platform (Windows/Mac/Linux)
- ✅ URL parsing
- ✅ File download
- ✅ Connection management

---

## Installing Prerequisites

### Python

**Windows:**
1. Download from [python.org](https://www.python.org/downloads/)
2. Run installer, check "Add Python to PATH"

**Mac:**
```bash
brew install python3
```

**Linux:**
```bash
sudo apt install python3  # Ubuntu/Debian
sudo yum install python3  # RHEL/CentOS
```

### pyftpdlib

```bash
pip install pyftpdlib      # Windows
pip3 install pyftpdlib     # Mac/Linux
```

---

## Troubleshooting

### "Connection refused"
- Make sure FTP server is running
- Check firewall isn't blocking port 21

### "pyftpdlib not found"
- Run: `pip install pyftpdlib` (Windows)
- Run: `pip3 install pyftpdlib` (Mac/Linux)

### "Permission denied" on port 21
- Windows: Run Command Prompt as Administrator
- Mac/Linux: Use `sudo` or change to port 2121

---

## Files Created for Demo

1. **Demo application**: `src-tauri/examples/ftp_demo.rs`
2. **Detailed guide**: `src-tauri/examples/README_FTP_DEMO.md`
3. **Prerequisite checkers**:
   - `src-tauri/examples/check_ftp_setup.ps1` (Windows)
   - `src-tauri/examples/check_ftp_setup.sh` (Mac/Linux)
4. **Verification script**: `test_ftp.ps1`
5. **Proof document**: `FTP_IMPLEMENTATION_PROOF.md`

---

## Quick Commands Cheat Sheet

```bash
# Check if ready
.\src-tauri\examples\check_ftp_setup.ps1         # Windows
bash src-tauri/examples/check_ftp_setup.sh       # Mac/Linux

# Verify implementation
.\test_ftp.ps1                                    # Windows only

# Run demo
# Terminal 1:
python -m pyftpdlib -p 21 -w -d C:\FTP_Test      # Windows
python3 -m pyftpdlib -p 21 -w -d /tmp/ftp_test   # Mac/Linux

# Terminal 2:
cd src-tauri
cargo run --example ftp_demo local
```