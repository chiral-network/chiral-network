# Git Setup Instructions for Chiral Network

This repository contains the Chiral Network application - a decentralized P2P file sharing platform.

## Quick Setup

1. **Initialize the repository** (if not already done):

```bash
cd /Users/shuai/workspace/brooknet/chiral-network/chiral-app
git init
```

2. **Configure your git identity**:

```bash
git config user.name "Your Name"
git config user.email "your.email@example.com"
```

3. **Add all files to staging**:

```bash
git add .
```

4. **Create the initial commit**:

```bash
git commit -m "Initial commit: Chiral Network - Decentralized P2P File Sharing Platform"
```

5. **Add the GitHub remote**:

```bash
git remote add origin git@github.com:chiral-network/chiral-network.git
```

6. **Push to GitHub**:

```bash
git branch -M main
git push -u origin main
```

## Alternative: Use the setup script

We've created a setup script that does all of the above:

```bash
chmod +x setup-git.sh
./setup-git.sh
```

Then push to GitHub:

```bash
git push -u origin main
```

## Project Structure

- `/src` - Svelte application source code
- `/src-tauri` - Tauri desktop application configuration
- `/docs` - Comprehensive documentation
- `/design-docs` - Original design documents from various teams
- `/public` - Static assets

## Important Notes

- Make sure you have SSH keys set up with GitHub
- Update the git user.name and user.email before committing
- The repository uses Chain ID 98765 for the Ethereum network
- All documentation is in the `/docs` folder
