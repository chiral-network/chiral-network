# Contributing to Chiral Network

Thank you for your interest in contributing to Chiral Network! This guide will help you get started.

## Code of Conduct

### Our Standards

- Be respectful and inclusive
- Welcome newcomers and help them learn
- Focus on constructive criticism
- Assume good intentions
- No harassment, discrimination, or abuse

## What Can You Contribute?

### Code Contributions
- Bug fixes
- New features (aligned with project goals)
- Performance improvements
- Test coverage
- Code refactoring

### Documentation
- Fix typos and errors
- Improve clarity
- Add examples
- Translate to other languages
- Write tutorials

### Design
- UI/UX improvements
- Icons and graphics
- Accessibility enhancements
- Responsive design

### Testing
- Write tests
- Report bugs
- Test on different platforms
- Performance testing

## What NOT to Contribute

**Please avoid**:
- ❌ Centralized market features
- ❌ Commercial tracking systems
- ❌ Features that enable piracy
- ❌ Global file search/discovery
- ❌ Social features (likes, comments)

## Getting Started

### 1. Set Up Development Environment

Follow the [Developer Setup Guide](developer-setup.md) to:
- Install prerequisites
- Clone repository
- Install dependencies
- Run development server

### 2. Find Something to Work On

**Good First Issues**:
- Look for issues labeled `good first issue`
- Check `help wanted` label
- Review documentation todos

**Areas Needing Help**:
- Translation improvements
- Test coverage
- Documentation
- Bug fixes

### 3. Create an Issue

Before starting work:
1. Search existing issues to avoid duplicates
2. Create new issue describing:
   - What you want to add/fix
   - Why it's needed
   - How you plan to implement it
3. Wait for maintainer feedback
4. Get approval before starting large changes

## Development Workflow

### 1. Fork and Clone

```bash
# Fork on GitHub, then clone your fork
git clone https://github.com/YOUR_USERNAME/chiral-network.git
cd chiral-network

# Add upstream remote
git remote add upstream https://github.com/Aery1e/chiral-network.git
```

### 2. Create a Branch

```bash
# Update main branch
git checkout main
git pull upstream main

# Create feature branch
git checkout -b feature/my-feature
```

Branch naming:
- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation
- `refactor/` - Code refactoring
- `test/` - Test additions

### 3. Make Changes

**Code Guidelines**:
- Follow existing code style
- Use TypeScript for new code
- Add JSDoc comments
- Write tests for new features
- Update documentation

**Commit Guidelines**:
```bash
# Use conventional commits
git commit -m "feat: add reputation filtering"
git commit -m "fix: resolve download queue bug"
git commit -m "docs: update NAT traversal guide"
```

Commit types:
- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation
- `style:` - Formatting
- `refactor:` - Code restructuring
- `test:` - Tests
- `chore:` - Maintenance
- `perf:` - Performance

### 4. Test Your Changes

```bash
# Run type checking
npm run check

# Run tests
npm test

# Test in dev mode
npm run tauri:dev

# Build to verify
npm run build
npm run tauri:build
```

### 5. Update Documentation

If your change affects:
- **User features**: Update [User Guide](user-guide.md)
- **Developer APIs**: Update [API Documentation](api-documentation.md)
- **Architecture**: Update [Architecture](architecture.md)
- **Setup process**: Update [Developer Setup](developer-setup.md)

### 6. Submit Pull Request

```bash
# Push to your fork
git push origin feature/my-feature
```

Then on GitHub:
1. Create Pull Request from your fork
2. Fill out PR template
3. Link related issues
4. Wait for review

## Pull Request Guidelines

### PR Title

Use conventional commit format:
```
feat: add bandwidth scheduling UI
fix: resolve peer connection timeout
docs: improve installation instructions
```

### PR Description

Include:
- **What** changed
- **Why** it was needed
- **How** it was implemented
- **Testing** performed
- **Screenshots** (for UI changes)
- **Breaking changes** (if any)

Example:
```markdown
## What
Adds bandwidth scheduling feature to Settings page

## Why
Users requested ability to limit bandwidth during specific hours

## How
- Added BandwidthScheduleEntry interface
- Created bandwidthScheduler service
- Added UI components for schedule management
- Integrated with existing bandwidth limits

## Testing
- [ ] Manual testing on macOS
- [ ] Tested schedule activation/deactivation
- [ ] Verified settings persistence
- [ ] Added unit tests

## Screenshots
![Settings page](url-to-screenshot)
```

### Review Process

1. **Automated checks** must pass:
   - TypeScript compilation
   - ESLint
   - Tests
   - Build

2. **Code review** by maintainer:
   - Code quality
   - Adherence to guidelines
   - Test coverage
   - Documentation

3. **Feedback** incorporation:
   - Address review comments
   - Make requested changes
   - Push updates

4. **Approval** and merge

## Code Style Guide

### TypeScript

```typescript
// Use interfaces for objects
interface FileMetadata {
  hash: string;
  name: string;
  size: number;
}

// Use type for unions
type Status = 'pending' | 'active' | 'completed';

// Explicit return types
function processFile(file: File): Promise<string> {
  // Implementation
}

// JSDoc comments
/**
 * Calculate composite reputation score for a peer
 * @param metrics - Peer performance metrics
 * @returns Reputation score between 0 and 1
 */
function calculateScore(metrics: PeerMetrics): number {
  // Implementation
}
```

### Svelte

```svelte
<script lang="ts">
  // Imports at top
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';

  // Props with types
  interface Props {
    fileName: string;
    size: number;
  }

  let { fileName, size }: Props = $props();

  // Reactive state
  let progress = $state(0);

  // Lifecycle
  onMount(() => {
    // Initialization
  });
</script>

<!-- Template -->
<div class="container">
  <h1>{$t('files.title')}</h1>
  <p>{fileName} - {size} bytes</p>
</div>

<!-- Styles (if component-specific) -->
<style>
  .container {
    padding: 1rem;
  }
</style>
```

### Rust

```rust
// Use standard formatting (rustfmt)
// Add documentation comments
/// Process a file for upload
///
/// # Arguments
/// * `path` - Path to the file
///
/// # Returns
/// File hash as a string
pub fn process_file(path: &str) -> Result<String, Error> {
    // Implementation
}

// Use Result for error handling
// Avoid unwrap() in production code
```

### CSS/Tailwind

```svelte
<!-- Prefer Tailwind classes -->
<div class="flex items-center gap-4 p-6 bg-white rounded-lg shadow">
  <!-- Content -->
</div>

<!-- Custom CSS only when necessary -->
<style>
  .custom-gradient {
    background: linear-gradient(to right, #667eea, #764ba2);
  }
</style>
```

## Translation Contributions

### Adding Translations

1. **Choose language** (en, es, ru, zh, ko)
2. **Edit locale file**: `src/locales/[lang].json`
3. **Translate all keys**
4. **Test in application**
5. **Submit PR**

See [i18n Guide](i18n.md) for details.

## Reporting Bugs

### Before Reporting

1. Check existing issues
2. Try latest version
3. Verify it's reproducible
4. Gather information

### Bug Report Template

```markdown
## Description
Clear description of the bug

## Steps to Reproduce
1. Go to '...'
2. Click on '...'
3. See error

## Expected Behavior
What should happen

## Actual Behavior
What actually happens

## Environment
- OS: [e.g., macOS 14.0]
- Version: [e.g., 0.1.0]
- Node: [e.g., 18.17.0]

## Screenshots
If applicable

## Additional Context
Any other relevant information
```

## Feature Requests

### Before Requesting

1. Check existing issues/discussions
2. Ensure alignment with project goals
3. Consider if it fits non-commercial focus

### Feature Request Template

```markdown
## Problem
What problem does this solve?

## Proposed Solution
How would you solve it?

## Alternatives Considered
Other approaches you've thought about

## Additional Context
Mockups, examples, etc.
```

## Review Guidelines

### For Reviewers

**When reviewing**:
- Be kind and constructive
- Explain reasoning
- Suggest alternatives
- Approve when ready

**Check for**:
- Code quality
- Test coverage
- Documentation updates
- Breaking changes
- Security implications

### For Contributors

**When receiving feedback**:
- Be open to suggestions
- Ask questions if unclear
- Make requested changes promptly
- Thank reviewers

## Community

### Communication Channels

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: General questions and ideas
- **Zulip**: Real-time chat and support

### Getting Help

- Read documentation first
- Search existing issues
- Ask in community channels
- Be patient and respectful

## Recognition

Contributors are recognized in:
- Git commit history
- GitHub contributors page
- Release notes
- Project README

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (MIT License).

## Questions?

If you have questions about contributing:
1. Read this guide thoroughly
2. Check [Developer Setup](developer-setup.md)
3. Search existing issues
4. Ask in GitHub Discussions
5. Join Zulip community

---

Thank you for contributing to Chiral Network! 🙏
