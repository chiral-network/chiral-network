#!/bin/bash

echo "========================================="
echo "Chiral Network - Upgrade to Svelte 5 & Tauri 2.0 Stable"
echo "========================================="
echo ""

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

# Check if npm is installed
if ! command -v npm &> /dev/null; then
    print_error "npm is not installed. Please install Node.js and npm first."
    exit 1
fi

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    print_error "Cargo is not installed. Please install Rust first."
    print_warning "Install Rust from: https://rustup.rs/"
    exit 1
fi

echo "Starting upgrade process..."
echo ""

# Step 1: Clean old dependencies
print_status "Cleaning old dependencies..."
rm -rf node_modules package-lock.json
rm -rf src-tauri/target

# Step 2: Install new Node dependencies
print_status "Installing updated Node.js dependencies..."
npm install

if [ $? -ne 0 ]; then
    print_error "Failed to install Node.js dependencies"
    exit 1
fi

# Step 3: Build Tauri dependencies
print_status "Building Tauri/Rust dependencies..."
cd src-tauri
cargo build

if [ $? -ne 0 ]; then
    print_error "Failed to build Rust dependencies"
    cd ..
    exit 1
fi
cd ..

# Step 4: Optional - Run Svelte 5 migration
echo ""
print_warning "Would you like to automatically migrate to Svelte 5 syntax?"
print_warning "This is optional - your code will work without migration."
echo "Run migration? (y/N): "
read -r response

if [[ "$response" =~ ^([yY][eE][sS]|[yY])$ ]]; then
    print_status "Running Svelte 5 migration..."
    npx sv migrate svelte-5
    
    if [ $? -eq 0 ]; then
        print_status "Migration completed successfully!"
    else
        print_warning "Migration had some issues. Please review the changes."
    fi
else
    print_status "Skipping Svelte 5 migration. You can run 'npx sv migrate svelte-5' later."
fi

echo ""
echo "========================================="
echo "Upgrade Complete!"
echo "========================================="
echo ""
print_status "Successfully updated to:"
echo "  • Svelte 5.0"
echo "  • Tauri 2.1 (stable)"
echo "  • TypeScript 5.7"
echo "  • All related dependencies"
echo ""
echo "Next steps:"
echo "1. Run 'npm run tauri dev' to test the application"
echo "2. Check MIGRATION.md for detailed changes"
echo "3. Test all functionality thoroughly"
echo ""
print_warning "If you encounter issues, check MIGRATION.md for troubleshooting"