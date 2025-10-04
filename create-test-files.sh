#!/bin/bash

echo "🔧 Creating Test Files for Multi-Source Downloads"
echo "================================================"

# Create test directory
mkdir -p test-files

# Create a 2MB test file (should trigger multi-source download)
echo "Creating large test file (2MB)..."
dd if=/dev/urandom of=test-files/large-test-file.bin bs=1024 count=2048 2>/dev/null
echo "✅ Created: large-test-file.bin (2MB)"

# Create a medium test file (500KB)
echo "Creating medium test file (500KB)..."
dd if=/dev/urandom of=test-files/medium-test-file.bin bs=1024 count=512 2>/dev/null
echo "✅ Created: medium-test-file.bin (500KB)"

# Create a small test file (100KB)
echo "Creating small test file (100KB)..."
dd if=/dev/urandom of=test-files/small-test-file.bin bs=1024 count=100 2>/dev/null
echo "✅ Created: small-test-file.bin (100KB)"

# Create a text file for easy verification
echo "Creating text test file..."
cat > test-files/test-document.txt << 'EOF'
# Multi-Source Download Test Document

This is a test document to verify multi-source download functionality.
It contains some sample text that can be easily verified after download.

Test Information:
- Created: $(date)
- Purpose: Multi-source download testing
- Size: Small text file

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor 
incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis 
nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.

This file should be downloaded using single-peer mode due to its small size.
The large binary files should trigger multi-source downloads.

End of test document.
EOF

echo "✅ Created: test-document.txt"

echo ""
echo "📁 Test files created in test-files/ directory:"
ls -lh test-files/
echo ""
echo "🎯 Testing Strategy:"
echo "• large-test-file.bin (2MB) → Should trigger multi-source download"
echo "• medium-test-file.bin (500KB) → Should use single-peer download"
echo "• small-test-file.bin (100KB) → Should use single-peer download"
echo "• test-document.txt → Easy to verify content after download"