# Developer Setup

Guide for setting up a development environment for Chiral Network.

## Prerequisites

### Required Software

- **Node.js** 18+ and npm
- **Rust** 1.70+ (for Tauri backend)
- **Git** for version control

### Platform-Specific Requirements

#### macOS

```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install Homebrew (if not installed)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Linux (Ubuntu/Debian)

```bash
# Update package list
sudo apt update

# Install dependencies
sudo apt install -y \
  build-essential \
  curl \
  wget \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libwebkit2gtk-4.0-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Node.js (via nvm)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
nvm install 18
nvm use 18
```

#### Windows

```powershell
# Install Chocolatey package manager
Set-ExecutionPolicy Bypass -Scope Process -Force
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))

# Install dependencies
choco install nodejs rust visualstudio2022-workload-vctools

# Install WebView2 (if not already installed)
# Download from: https://developer.microsoft.com/en-us/microsoft-edge/webview2/
```

## Clone Repository

```bash
git clone https://github.com/chiral-network/chiral-network.git
cd chiral-network
```

## Install Dependencies

```bash
# Install Node.js dependencies
npm install

# Verify Rust installation
rustc --version
cargo --version

# Install Tauri CLI (optional, for CLI usage)
cargo install tauri-cli
```

## Development Environment

### IDE Recommendations

#### VS Code (Recommended)

Install these extensions:

- **Svelte for VS Code** - Svelte language support
- **Tailwind CSS IntelliSense** - Tailwind autocomplete
- **rust-analyzer** - Rust language server
- **Tauri** - Tauri development tools
- **ESLint** - JavaScript linting
- **Prettier** - Code formatting

#### WebStorm

- Built-in Svelte support
- TypeScript support
- Install Rust plugin

### VS Code Settings

Create `.vscode/settings.json`:

```json
{
  "editor.formatOnSave": true,
  "editor.defaultFormatter": "esbenp.prettier-vscode",
  "[svelte]": {
    "editor.defaultFormatter": "svelte.svelte-vscode"
  },
  "svelte.enable-ts-plugin": true,
  "typescript.tsdk": "node_modules/typescript/lib",
  "files.associations": {
    "*.svelte": "svelte"
  }
}
```

## Running the Application

### Development Mode

#### Web Development Server (Frontend Only)

```bash
npm run dev
# Opens http://localhost:5173
# Hot reload enabled
```

#### Tauri Development (Full Desktop App)

```bash
# Start the WebRTC signaling server (required for WebRTC connections)
npm run signaling-server

# In a separate terminal, start the application
npm run tauri:dev
# Builds frontend + Rust backend
# Opens desktop application
# Hot reload for frontend
```

**Note**: The WebRTC signaling server must be running on port 3000 for WebRTC peer connections to work. Start it with `npm run signaling-server` before running the application.

### Development Workflow

1. **Start dev server**: `npm run tauri:dev`
2. **Make changes** to Svelte/TypeScript files
3. **Browser auto-reloads** on save
4. **Rust changes require** manual rebuild (Ctrl+C and restart)

### Debugging

#### Frontend Debugging

- Open DevTools in Tauri window (Right-click → Inspect)
- Use browser console for logs
- Svelte DevTools extension available

#### Backend Debugging

```bash
# Enable Rust debug logs
RUST_LOG=debug npm run tauri:dev

# Specific module logs
RUST_LOG=chiral_network=debug npm run tauri:dev
```

## Project Structure

```
chiral-network/
├── src/                      # Frontend source
│   ├── lib/                  # Svelte libraries
│   │   ├── components/       # Reusable components
│   │   ├── diagnostics/      # Diagnostic tools
│   │   ├── services/         # Business logic services
│   │   ├── stores/           # Additional state stores
│   │   ├── types/            # TypeScript type definitions
│   │   ├── utils/            # Utility functions
│   │   └── wallet/           # HD wallet implementation
│   ├── pages/                # Application pages
│   ├── routes/               # Special routes
│   ├── styles/               # Global styles
│   ├── types/                # Global type definitions
│   ├── locales/              # i18n translation files
│   ├── i18n/                 # i18n configuration
│   ├── App.svelte            # Main app component
│   └── main.ts               # Entry point
├── src-tauri/                # Rust backend
│   ├── src/                  # Rust source code
│   │   ├── commands/         # Tauri command handlers
│   │   ├── net/              # Network modules
│   │   └── ...               # Other Rust modules
│   ├── capabilities/         # Tauri capabilities
│   ├── icons/                # Application icons
│   ├── tests/                # Rust tests
│   ├── build.rs              # Build script
│   ├── Cargo.toml            # Rust dependencies
│   └── tauri.conf.json       # Tauri configuration
├── relay/                    # Relay server implementation
├── storage/                  # Storage module
├── docs/                     # Documentation
├── tests/                    # Frontend tests
├── genesis.json              # Blockchain genesis configuration
├── index.html                # HTML entry point
├── package.json              # Node.js dependencies
├── vite.config.ts            # Vite configuration
├── vitest.config.ts          # Vitest configuration
├── svelte.config.js          # Svelte configuration
├── tailwind.config.js        # Tailwind CSS config
├── postcss.config.js         # PostCSS configuration
├── tsconfig.json             # TypeScript configuration
└── tsconfig.node.json        # TypeScript Node configuration
```

## Common Development Tasks

### Adding a New Page

1. Create component: `src/pages/MyPage.svelte`
2. Import in `App.svelte`
3. Add route configuration
4. Add to navigation menu
5. Add translations to locale files

### Creating a New Service

1. Create file: `src/lib/services/myService.ts`
2. Export service class or functions
3. Import in components that need it
4. Add tests: `tests/myService.test.ts`

### Adding a Tauri Command

1. Add function in `src-tauri/src/commands/` (or create new module):

```rust
#[tauri::command]
pub fn my_command(param: String) -> Result<String, String> {
    // Implementation
    Ok("Result".to_string())
}
```

2. Register in `src-tauri/src/main.rs`:

```rust
tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![my_command])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
```

3. Call from frontend:

```typescript
import { invoke } from "@tauri-apps/api/core";

const result = await invoke<string>("my_command", { param: "value" });
```

## Testing

### Unit Tests

```bash
# Run all tests
npm test

# Run tests in watch mode
npm run test:watch

# Run with coverage
npm test -- --coverage
```

### Test Structure

```typescript
// tests/myService.test.ts
import { describe, it, expect } from "vitest";
import { myService } from "../src/lib/services/myService";

describe("myService", () => {
  it("should do something", () => {
    const result = myService.doSomething();
    expect(result).toBe(expected);
  });
});
```

### Integration Tests

Located in `tests/` directory:

- `peerSelection.test.ts` - Peer selection logic
- `multi-source-download.test.ts` - Download functionality
- `mining.test.ts` - Mining operations
- `dhtHelpers.test.ts` - DHT utilities

## Type Checking

```bash
# Run TypeScript type checker
npm run check

# Watch mode
npm run check -- --watch
```

## Linting & Formatting

### ESLint

```bash
# Lint all files
npm run lint

# Auto-fix issues
npm run lint -- --fix
```

### Prettier

```bash
# Format all files
npm run format

# Check formatting
npm run format -- --check
```

## Building

### Development Build

```bash
npm run build
```

### Production Build

```bash
# Build web version
npm run build

# Build desktop application
npm run tauri:build
```

Build outputs:

- **Web**: `dist/` directory
- **Desktop**: `src-tauri/target/release/bundle/`

## Environment Variables

Create `.env` file in project root:

```bash
# Development
VITE_DEV_MODE=true
VITE_API_URL=http://localhost:3000

# Backend
RUST_LOG=debug
RUST_BACKTRACE=1
```

Access in frontend:

```typescript
const apiUrl = import.meta.env.VITE_API_URL;
```

## Troubleshooting

### Common Issues

#### "Command not found: tauri"

```bash
# Install Tauri CLI
cargo install tauri-cli
# Or use via npm
npm run tauri
```

#### "WebView2 not found" (Windows)

Download and install WebView2 Runtime:
https://developer.microsoft.com/en-us/microsoft-edge/webview2/

#### Port 5173 already in use

```bash
# Kill process using port
# macOS/Linux
lsof -ti:5173 | xargs kill
# Windows
netstat -ano | findstr :5173
taskkill /PID <PID> /F
```

#### Rust compilation errors

```bash
# Clean and rebuild
cd src-tauri
cargo clean
cargo build
```

#### Node modules issues

```bash
# Clean install
rm -rf node_modules package-lock.json
npm install
```

### Getting Help

- Check [GitHub Issues](https://github.com/chiral-network/chiral-network/issues)
- Review [Tauri Documentation](https://tauri.app/v1/guides/)
- Read [Svelte Documentation](https://svelte.dev/docs)
- Join community on Zulip

## Development Best Practices

### Code Style

- Follow existing patterns in codebase
- Use TypeScript for all new code
- Add JSDoc comments to functions
- Keep components small and focused
- Write tests for new features

### Git Workflow

```bash
# Create feature branch
git checkout -b feature/my-feature

# Make changes and commit
git add .
git commit -m "feat: add my feature"

# Push and create PR
git push origin feature/my-feature
```

### Commit Messages

Follow conventional commits:

- `feat:` New feature
- `fix:` Bug fix
- `docs:` Documentation
- `style:` Formatting
- `refactor:` Code restructuring
- `test:` Adding tests
- `chore:` Maintenance

## Performance Profiling

### Frontend Performance

```typescript
// Use Svelte DevTools
// Open DevTools → Svelte tab

// Or manual timing
console.time("operation");
// ... code ...
console.timeEnd("operation");
```

### Backend Performance

```rust
use std::time::Instant;

let start = Instant::now();
// ... code ...
let duration = start.elapsed();
println!("Time elapsed: {:?}", duration);
```

## Continuous Integration

GitHub Actions workflow (`.github/workflows/test.yml`):

- Runs on push/PR
- Tests all platforms (macOS, Linux, Windows)
- Runs lint, type check, and tests
- Builds application

## See Also

- [Implementation Guide](implementation-guide.md) - Development workflows
- [Contributing Guide](contributing.md) - How to contribute
- [API Documentation](api-documentation.md) - API reference
- [Architecture](architecture.md) - System architecture
