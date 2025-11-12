#!/bin/bash
# FTP Demo Prerequisites Check
# Works on Mac/Linux

echo "==================================="
echo "  FTP Demo Prerequisites Check"
echo "==================================="
echo ""

# Check Python
echo "Checking Python installation..."
if command -v python3 &> /dev/null; then
    PYTHON_VERSION=$(python3 --version)
    echo "  ✓ Python found: $PYTHON_VERSION"
    PYTHON_CMD="python3"
elif command -v python &> /dev/null; then
    PYTHON_VERSION=$(python --version)
    echo "  ✓ Python found: $PYTHON_VERSION"
    PYTHON_CMD="python"
else
    echo "  ✗ Python NOT found"
    echo "    Install: sudo apt install python3  (Ubuntu/Debian)"
    echo "             brew install python3       (Mac)"
    PYTHON_CMD=""
fi
echo ""

# Check pyftpdlib
if [ -n "$PYTHON_CMD" ]; then
    echo "Checking pyftpdlib installation..."
    if $PYTHON_CMD -c "import pyftpdlib" 2>/dev/null; then
        echo "  ✓ pyftpdlib found"
    else
        echo "  ✗ pyftpdlib NOT found"
        echo "    Install: pip3 install pyftpdlib"
    fi
    echo ""
fi

# Check Rust/Cargo
echo "Checking Rust installation..."
if command -v cargo &> /dev/null; then
    CARGO_VERSION=$(cargo --version)
    echo "  ✓ Cargo found: $CARGO_VERSION"
else
    echo "  ✗ Cargo NOT found"
    echo "    Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi
echo ""

echo "==================================="
echo "  Recommendation"
echo "==================================="
echo ""

if [ -n "$PYTHON_CMD" ] && $PYTHON_CMD -c "import pyftpdlib" 2>/dev/null; then
    echo "✓ All prerequisites met!"
    echo ""
    echo "To run FTP demo:"
    echo "  Terminal 1: $PYTHON_CMD -m pyftpdlib -p 2121 -w -d /tmp/ftp_test"
    echo "  Terminal 2: cargo run --example ftp_demo local"
else
    echo "Some prerequisites missing."
    echo ""
    echo "Option 1: Install missing prerequisites (see above)"
    echo "Option 2: Use public FTP server (no setup):"
    echo "  cargo run --example ftp_demo"
fi
echo ""