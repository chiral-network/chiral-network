#!/bin/bash

echo "🧪 Multi-Source Download Testing Script"
echo "======================================="

# Check Node.js version
echo "1. Checking Node.js version..."
node_version=$(node --version | cut -d'v' -f2)
required_major=20

if [ $(echo $node_version | cut -d'.' -f1) -lt $required_major ]; then
    echo "❌ Node.js version $node_version is too old. Required: 20.19+"
    echo "Please upgrade Node.js first!"
    exit 1
fi

echo "✅ Node.js version: $node_version"

# Install dependencies
echo "2. Installing dependencies..."
npm install

# Check TypeScript compilation
echo "3. Checking TypeScript compilation..."
npm run check
if [ $? -ne 0 ]; then
    echo "❌ TypeScript compilation failed"
    exit 1
fi

echo "✅ TypeScript compilation successful"

# Check Rust compilation
echo "4. Checking Rust compilation..."
cd src-tauri
cargo check --quiet
if [ $? -ne 0 ]; then
    echo "❌ Rust compilation failed"
    exit 1
fi

echo "✅ Rust compilation successful"
cd ..

# Build the application
echo "5. Building application..."
npm run tauri build

if [ $? -eq 0 ]; then
    echo "✅ Build successful! Ready for testing."
    echo ""
    echo "🚀 Testing Instructions:"
    echo "1. Run the built application from target/release/"
    echo "2. Or use: npm run tauri dev (for development mode)"
else
    echo "❌ Build failed. Check logs above."
fi