#!/bin/bash

# Chiral Network - Tunnel Tools Installation Script
# This script installs various tunnel providers for HTTP file sharing

echo "🌐 Installing tunnel tools for Chiral Network..."

# Check if we're on macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "🍎 Detected macOS"
    
    # Check if Homebrew is installed
    if ! command -v brew &> /dev/null; then
        echo "❌ Homebrew not found. Please install Homebrew first:"
        echo "   /bin/bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
        exit 1
    fi
    
    echo "📦 Installing tunnel tools via Homebrew..."
    
    # Install ngrok
    if ! command -v ngrok &> /dev/null; then
        echo "Installing ngrok..."
        brew install ngrok/ngrok/ngrok
    else
        echo "✅ ngrok already installed"
    fi
    
    # Install cloudflared
    if ! command -v cloudflared &> /dev/null; then
        echo "Installing cloudflared..."
        brew install cloudflared
    else
        echo "✅ cloudflared already installed"
    fi
    
    # Install bore (via cargo)
    if ! command -v bore &> /dev/null; then
        echo "Installing bore..."
        if command -v cargo &> /dev/null; then
            cargo install bore-cli
        else
            echo "⚠️  Cargo not found. Install Rust first: https://rustup.rs/"
            echo "   Then run: cargo install bore-cli"
        fi
    else
        echo "✅ bore already installed"
    fi
    
    # Install localtunnel (fallback)
    if ! command -v lt &> /dev/null; then
        echo "Installing localtunnel..."
        npm install -g localtunnel
    else
        echo "✅ localtunnel already installed"
    fi

elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    echo "🐧 Detected Linux"
    
    # Check if we have a package manager
    if command -v apt &> /dev/null; then
        echo "📦 Using apt package manager..."
        
        # Update package list
        sudo apt update
        
        # Install cloudflared
        if ! command -v cloudflared &> /dev/null; then
            echo "Installing cloudflared..."
            wget -q https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
            sudo dpkg -i cloudflared-linux-amd64.deb
            rm cloudflared-linux-amd64.deb
        else
            echo "✅ cloudflared already installed"
        fi
        
    elif command -v yum &> /dev/null; then
        echo "📦 Using yum package manager..."
        # Add similar installation logic for yum-based systems
    fi
    
    # Install ngrok
    if ! command -v ngrok &> /dev/null; then
        echo "Installing ngrok..."
        curl -s https://ngrok-agent.s3.amazonaws.com/ngrok.asc | sudo tee /etc/apt/trusted.gpg.d/ngrok.asc >/dev/null
        echo "deb https://ngrok-agent.s3.amazonaws.com buster main" | sudo tee /etc/apt/sources.list.d/ngrok.list
        sudo apt update && sudo apt install ngrok
    else
        echo "✅ ngrok already installed"
    fi
    
    # Install bore
    if ! command -v bore &> /dev/null; then
        echo "Installing bore..."
        if command -v cargo &> /dev/null; then
            cargo install bore-cli
        else
            echo "⚠️  Cargo not found. Install Rust first: https://rustup.rs/"
            echo "   Then run: cargo install bore-cli"
        fi
    else
        echo "✅ bore already installed"
    fi
    
    # Install localtunnel
    if ! command -v lt &> /dev/null; then
        echo "Installing localtunnel..."
        if command -v npm &> /dev/null; then
            npm install -g localtunnel
        else
            echo "⚠️  npm not found. Install Node.js first: https://nodejs.org/"
            echo "   Then run: npm install -g localtunnel"
        fi
    else
        echo "✅ localtunnel already installed"
    fi

else
    echo "❌ Unsupported operating system: $OSTYPE"
    echo "Please install tunnel tools manually:"
    echo "  - ngrok: https://ngrok.com/download"
    echo "  - cloudflared: https://github.com/cloudflare/cloudflared/releases"
    echo "  - bore: cargo install bore-cli"
    echo "  - localtunnel: npm install -g localtunnel"
    exit 1
fi

echo ""
echo "🎉 Tunnel tools installation complete!"
echo ""
echo "Available tunnel providers:"
echo "  🚀 ngrok - Most reliable, requires account"
echo "  ⚡ cloudflared - Fast and free from Cloudflare"
echo "  🔧 bore - Simple and lightweight"
echo "  🏠 self-hosted - Most private, requires port forwarding"
echo "  📡 localtunnel - Fallback option"
echo ""
echo "💡 Chiral Network will automatically try the best available provider!"
echo "   For maximum privacy, use 'self_hosted' and configure port forwarding."