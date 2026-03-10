# Application Pages

## Routing

Routing is configured in `src/App.svelte` using `@mateothegreat/svelte5-router`.

- Unauthenticated routes: `/wallet`
- Authenticated routes: `/network`, `/download`, `/drive`, `/chiraldrop`, `/hosting`, `/hosts`, `/mining`, `/account`, `/settings`, `/diagnostics`

## Wallet (`/wallet`)

- Create wallet with mnemonic verification
- Import wallet via mnemonic/private key
- Persists active wallet session

## Network (`/network`)

- Start/stop DHT
- View peer IDs, peer list, and network health
- Geth status visibility and chain connectivity info

## Download (`/download`)

- Search by file hash, magnet, or torrent
- Seeder list selection with seeder Elo display
- Speed-tier download flow with progress and history
- Handles same-node/local seeder fast path when available

## Drive (`/drive`)

- Folder/file CRUD
- Star, move, share, and visibility controls
- Seeding tab with protocol and price controls
- Publish to DHT without uploading file payload to remote storage
- Delete now removes both manifest entry and local stored file

## ChiralDrop (`/chiraldrop`)

- Direct peer transfer UX
- Accept/decline incoming file transfers
- Transfer history with encrypted local persistence support

## Hosting (`/hosting`)

- Manage hosted content lifecycle
- Site hosting controls and relay publication integration

## Hosts (`/hosts`)

- Marketplace host discovery and proposals
- Host participation controls (including publish/ad settings)
- Auto-accept threshold behavior based on Elo
- Agreement lifecycle and deposit/price controls

## Mining (`/mining`)

- CPU mining controls
- GPU mining controls and capability/status display
- Mined block visibility

## Account (`/account`)

- Wallet balance and send flow
- Transaction history
- Reputation section with Elo/rating data

## Settings (`/settings`)

- Theme and accent configuration
- Download/storage preferences
- Notification preferences

## Diagnostics (`/diagnostics`)

- Environment/network/storage/system checks
- Health/readiness surface for troubleshooting
