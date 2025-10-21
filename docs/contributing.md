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

### Design Documentation
- Add major parts 
- Fix critical issues
- Add protocol specifications
- Add examples and tutorials
- Fix minor issues


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

### UI/UX Improvements
- Icons and graphics
- Accessibility enhancements
- Responsive design

### Testing
- Write tests
- Report bugs
- Test on different platforms
- Performance testing

## Contribution Priorities

**IMPORTANT**: We prioritize contributions based on impact and type. Please follow this priority order:

### Quick Reference

| Priority | Type | Examples | Review Speed |
|----------|------|----------|--------------|
| **1 🔴** | **Major Design/Docs** | Architecture decisions, protocol specs, API contracts | Thorough review required |
| **2 🟠** | **Major Code/Critical Fixes** | Protocol implementations, security fixes, core features | Thorough review required |
| **3 🟡** | **Minor Design/Docs** | Examples, tutorials, typos, clarifications | Faster review |
| **4 🟢** | **Minor Code** | UI polish, code comments, small optimizations | Fastest review |

### Priority 1: Major Design/Documentation Work (HIGHEST) 🔴
- **Add major parts to design documentation**
  - Architecture decisions and system design
  - Protocol specifications and data flow diagrams
  - Major new features in design phase
  - API contracts and interfaces
- **Fix critical issues in documentation**
  - Incorrect architectural descriptions
  - Misleading protocol specifications
  - Security vulnerabilities in design
  - Breaking inconsistencies across docs

**Why highest priority?** Design decisions affect everything. Getting the design right before implementation saves significant rework and prevents architectural mistakes.

### Priority 2: Major Code Work or Critical Code Fixes 🟠
- **Add major parts to codebase**
  - Core protocol implementations (HTTP, WebTorrent, BitTorrent, ed2k)
  - Payment system integration
  - DHT and P2P networking features
  - Blockchain integration
- **Fix critical code issues**
  - Security vulnerabilities
  - Data corruption bugs
  - Payment calculation errors
  - Network protocol breaking issues
  - Crashes and system instability

**Why high priority?** Major features and critical bugs directly impact functionality and user experience.

### Priority 3: Minor Design/Documentation Improvements 🟡
- **Add minor parts to documentation**
  - Examples and tutorials
  - Clarifications and additional details
  - Diagrams and illustrations
  - FAQ entries
- **Fix small issues in documentation**
  - Typos and grammar
  - Formatting improvements
  - Dead links
  - Minor inconsistencies

**Why medium priority?** Documentation improvements help users but don't block development.

### Priority 4: Minor Code Improvements (LOWEST) 🟢
- **Add minor features**
  - UI polish and minor UX improvements
  - Non-critical convenience features
  - Additional logging and debugging aids
  - Code comments and inline documentation
- **Fix small code issues**
  - Non-critical UI bugs
  - Minor performance optimizations
  - Code style and formatting
  - Refactoring for readability

**Why lower priority?** While valuable, these can be addressed after critical design and implementation work.

### How to Choose What to Work On

1. **Check the Roadmap**: See [roadmap.md](roadmap.md) for current phase priorities
2. **Look for High-Priority Labels**: Issues labeled `priority: critical` or `priority: high`
3. **Ask First**: For major work, create an issue to discuss design before coding
4. **Design Before Code**: Always update documentation before implementing major features

### Workflow for Major Contributions

```
1. Design Phase (Priority 1) 🔴
   └─> Create issue → Discuss architecture → Update docs → Get approval

2. Implementation Phase (Priority 2) 🟠
   └─> Write code → Add tests → Update technical docs → Submit PR

3. Polish Phase (Priority 3-4) 🟡🟢
   └─> Add examples → Fix minor issues → Improve UX → Optimize
```

### Summary: Why This Priority Order?

```
┌─────────────────────────────────────────────────────────┐
│  Priority 1 🔴: Design Right (Architecture First)       │
│  ↓                                                       │
│  Priority 2 🟠: Build Right (Implementation)            │
│  ↓                                                       │
│  Priority 3 🟡: Document Right (Examples/Clarity)       │
│  ↓                                                       │
│  Priority 4 🟢: Polish (UX/Performance)                 │
└─────────────────────────────────────────────────────────┘

Design mistakes are expensive to fix after implementation.
Architecture docs guide all development decisions.
Get the design right first, then build, then polish.
```

**Remember**:
- ✅ Design before code (Priority 1 before Priority 2)
- ✅ Critical before nice-to-have (Priority 1-2 before Priority 3-4)
- ✅ Documentation updates required for all Priority 1-2 changes
- ✅ Ask maintainers if unsure about priority level

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

**Follow Priority Order** (see [Contribution Priorities](#contribution-priorities) above):
1. **Priority 1** 🔴: Major design/documentation work
2. **Priority 2** 🟠: Major code work or critical fixes
3. **Priority 3** 🟡: Minor documentation improvements
4. **Priority 4** 🟢: Minor code improvements

**Good First Issues**:
- Look for issues labeled `good first issue` (usually Priority 3-4)
- Check `help wanted` label
- Start with documentation (Priority 3) to learn the system

**Current High-Priority Areas** (Check [roadmap.md](roadmap.md)):
- Protocol documentation (Priority 1)
- Multi-protocol implementation (Priority 2)
- Critical bug fixes (Priority 2)
- Architecture documentation updates (Priority 1)

**Lower Priority Areas** (Good for newcomers):
- Translation improvements (Priority 3-4)
- Test coverage (Priority 4)
- UI polish (Priority 4)
- Code comments (Priority 4)

### 3. Create an Issue

Before starting work:
1. Search existing issues to avoid duplicates
2. Create new issue describing:
   - What you want to add/fix
   - Why it's needed
   - How you plan to implement it
   - **Priority level** (1-4 based on contribution priorities)
3. Wait for maintainer feedback
4. **CRITICAL for Priority 1-2**: Get design approval before coding
   - Discuss architecture in issue comments
   - Update design documentation first
   - Get maintainer sign-off on approach
5. For Priority 3-4: Approval recommended but less critical

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
- **Priority Level** (1-4) - Indicate contribution priority
- **What** changed
- **Why** it was needed
- **How** it was implemented
- **Testing** performed
- **Documentation Updates** (especially for Priority 1-2)
- **Screenshots** (for UI changes)
- **Breaking changes** (if any)

Example (Priority 2 - Major Code Work):
```markdown
## Priority
Priority 2: Major code implementation (BitTorrent protocol support)

## What
Adds BitTorrent protocol support to the multi-protocol file transfer system

## Why
Required for Phase 3 roadmap - enables efficient swarming and compatibility with existing BitTorrent ecosystem

## How
- Implemented BitTorrent protocol handler in ProtocolManager
- Added piece exchange logic with peer selection
- Integrated with existing DHT for peer discovery
- Ensured protocol-agnostic payment settlement

## Documentation Updates
- [ ] Updated architecture.md with BitTorrent implementation details
- [ ] Updated network-protocol.md with BitTorrent specifications
- [ ] Added examples to implementation-guide.md

## Testing
- [ ] Manual testing on macOS with multiple peers
- [ ] Tested piece exchange with different chunk sizes
- [ ] Verified payment settlement works correctly
- [ ] Added unit tests for protocol handler
- [ ] Tested interoperability with standard BitTorrent clients

## Breaking Changes
None - backward compatible with existing HTTP/WebTorrent implementations
```

Example (Priority 3 - Minor Documentation):
```markdown
## Priority
Priority 3: Minor documentation improvement

## What
Added examples and clarifications to NAT traversal documentation

## Why
Users reported confusion about Circuit Relay v2 configuration

## How
- Added step-by-step setup examples
- Created troubleshooting section
- Added diagrams for relay flow

## Testing
- [ ] Reviewed by technical writer
- [ ] Verified all links work
- [ ] Tested examples on fresh installation
```

### Review Process

**Review Priority**: PRs are reviewed in priority order (Priority 1 → Priority 4)

1. **Automated checks** must pass:
   - TypeScript compilation
   - ESLint
   - Tests
   - Build

2. **Code review** by maintainer (Priority-specific):
   - **Priority 1-2** (Design/Major Code):
     - Architecture alignment with docs
     - Design correctness
     - Breaking change impact
     - Documentation completeness
     - Code quality and test coverage
   - **Priority 3-4** (Minor improvements):
     - Code quality
     - Adherence to guidelines
     - No regression introduced

3. **Feedback** incorporation:
   - Address review comments
   - Make requested changes
   - Push updates

4. **Approval** and merge
   - Priority 1-2 may require multiple maintainer reviews
   - Priority 3-4 can be merged faster

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
