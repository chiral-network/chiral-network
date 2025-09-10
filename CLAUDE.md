# Chiral Network Development Guide

## Project Overview

Chiral Network is a BitTorrent-like P2P file sharing application built with Svelte, TypeScript, and Tauri. It implements a continuous seeding model where files are instantly available to the network, similar to BitTorrent but without any commercial/marketplace features to prevent misuse.

## Current Architecture

### Core Design Principles

1. **BitTorrent-Style Sharing**: Files immediately start seeding when added (no "upload" step)
2. **Non-Commercial**: No marketplace, pricing, or trading features
3. **Privacy-First**: Proxy support, optional encryption, anonymous mode
4. **Legitimate Use Only**: Designed for personal, educational, and organizational file sharing

### Technology Stack

- **Frontend**: Svelte 4 + TypeScript
- **Styling**: Tailwind CSS
- **Desktop**: Tauri 2 (Rust-based)
- **State Management**: Svelte stores
- **Icons**: Lucide Svelte
- **UI Components**: Custom components with Bits UI

## Key Implementation Details

### File Sharing Model

- Files are **instantly seeded** when added (no pending/uploaded distinction)
- Each file gets a unique hash (mock: `Qm...` format like IPFS)
- Files show real-time seeder/leecher counts
- Continuous seeding until manually removed
- No price fields or marketplace features

### Page Structure

1. **Download** (default page) - Unified download management with filters
2. **Upload** - Actually "Shared Files" - instant seeding interface
3. **Network** - Peer discovery and network statistics
4. **Mining** - CPU mining for network security (proof-of-work)
5. **Proxy** - Privacy routing configuration
6. **Analytics** - Usage statistics and performance metrics
7. **Account** - Wallet management (for mining rewards only)
8. **Settings** - Comprehensive configuration options

### State Management (`src/lib/stores.ts`)

```typescript
- files: All files (downloading, seeding, completed)
- downloadQueue: Files waiting to download
- peers: Connected network peers
- proxyNodes: Available proxy servers
- networkStats: Global network statistics
- wallet: User wallet for mining rewards
```

## Recent Design Decisions

### UI/UX Improvements

1. **Removed Large Drop Zones**: Replaced with compact "Add Files" button
2. **Unified Lists**: Merged multiple lists into single views with filters
3. **Drag Anywhere**: Entire cards accept drag-and-drop
4. **Instant Actions**: Files start seeding immediately when added

### Removed Features (Anti-Piracy)

- ❌ Search page (could enable finding copyrighted content)
- ❌ Market page (no commercial transactions)
- ❌ Bundles page (no selling file packages)
- ❌ Pricing fields (no monetization)
- ❌ Ratings/reviews (no marketplace features)

## Development Guidelines

### When Adding Features

1. **No Commercial Elements**: Never add pricing, trading, or marketplace features
2. **Privacy First**: Always consider user privacy and anonymity
3. **Legitimate Use**: Design for legal file sharing use cases only
4. **BitTorrent Model**: Files should seed continuously, not "upload once"

### Code Style

- Use TypeScript for type safety
- Follow existing Svelte patterns
- Keep components small and focused
- Use Tailwind classes for styling

### Testing Approach

- Test with mock data first
- Ensure UI works without backend
- Verify drag-and-drop functionality
- Test responsive design

## Common Tasks

### Adding a New Page

1. Create component in `src/pages/`
2. Import in `App.svelte`
3. Add to navigation menu
4. Update route handling
5. Add icon from Lucide

### Modifying Stores

1. Update interfaces in `stores.ts`
2. Adjust mock data if needed
3. Update dependent components
4. Test state reactivity

## Future Enhancements (Allowed)

### Phase 2 Priorities

- [ ] Real P2P networking with libp2p
- [ ] Actual file encryption
- [ ] DHT implementation
- [ ] WebRTC data channels
- [ ] Real mining algorithm

### Phase 3 Possibilities

- [ ] File versioning system
- [ ] Bandwidth scheduling
- [ ] Mobile app version
- [ ] Hardware wallet support
- [ ] IPFS compatibility

## What NOT to Implement

⚠️ **Never add these features:**

- Global file search/discovery
- Price fields or payment systems
- File marketplace or trading
- Content recommendations
- Social features (comments, likes)
- Advertising systems
- Analytics that could track users

## Security Considerations

- All file hashes should be deterministic
- Never log or expose private keys
- Sanitize all user inputs
- Use secure random for IDs
- Implement rate limiting
- Validate file sizes and types

## Performance Notes

- Lazy load large lists
- Use virtual scrolling for many items
- Debounce search inputs
- Cache computed values
- Minimize re-renders
- Optimize bundle size

## Deployment

```bash
# Development
npm run dev
npm run tauri dev

# Production build
npm run build
npm run tauri build

# Generate icons
npm run tauri icon path/to/icon.png
```

## Troubleshooting

### Common Issues

1. **Extra `</script>` tags**: Check Svelte files end correctly
2. **Import errors**: Ensure all pages are properly imported
3. **Drag-drop failing**: Verify event handlers are attached

### Debug Commands

```bash
# Check for syntax errors
npm run check

# Clean and rebuild
rm -rf node_modules dist
npm install
npm run build
```

## Contact & Support

For questions about design decisions or implementation details, refer to:

1. This CLAUDE.md file
2. README.md for user-facing documentation
3. Design documents in `/design-docs` folder
4. Git history for decision context

---

_Last Updated: Current Session_
_Focus: BitTorrent-like P2P sharing without commercial features_
