# Chiral Network

Chiral Network is a decentralized file-sharing desktop app. You can share files directly with other users, pay (or get paid) for downloads using the built-in CHI cryptocurrency, host files on the always-on CDN, and even mine CHI on your computer — all from a single app.

There's no central server deciding what can be shared or who can participate. Your files stay on your computer (or on the CDN if you choose), and transfers happen peer-to-peer.

## What You Can Do

- **Share files with anyone** — publish a file to the network and other users can find and download it.
- **Download files** — search by file hash and download from peers who are sharing it, paying a small CHI fee per megabyte.
- **Manage a personal Drive** — organize your local files and choose which ones to seed to the network.
- **Send files directly (ChiralDrop)** — one-to-one transfers with an optional price.
- **Host files on the CDN** — upload to an always-on server so your files remain available even when your computer is offline. You set the download price; others pay you.
- **Run a hosting marketplace** — browse peer hosts and CDN servers, sign hosting agreements, and earn CHI by hosting other users' files.
- **Publish a static site** — host an HTML/JS/CSS site through a relay or the CDN, claim a memorable name in the network's site directory, and let other peers find it by that name.
- **Sell folders, not just files** — share a whole folder under a single hash; buyers see the file list and total cost, pay once, and download the bundle.
- **Mine CHI** — use your CPU (or GPU on Linux/Windows) to mine blocks and earn 5 CHI per block.
- **Manage your wallet** — create a new wallet or import an existing one, back it up by email, view your balance and transaction history, and send CHI to other addresses.
- **Check your reputation** — every completed or failed transfer feeds into an Elo-style reputation score (0–100) that other users can see.

## Getting Started

### 1. Install

Download the installer for your platform from the releases page and install it like any normal desktop app. Chiral Network runs on **Windows**, **macOS**, and **Linux**.

### 2. First launch

When you open the app for the first time:

1. **Create a wallet.** You'll be given a recovery phrase — write it down and keep it somewhere safe. This is the only way to recover your wallet if you lose access. You can optionally receive an encrypted backup by email.
2. **Connect to the network.** The app automatically connects to the Chiral peer-to-peer network. You should see peers appear on the Network page within a few seconds.
3. **Get some CHI.** You'll need a small amount of CHI to download files (0.01 CHI per MB). You can either:
   - Mine some yourself on the Mining page, or
   - Receive CHI from another user who sends it to your wallet address.

### 3. Share or download a file

- **To share**, go to the **Drive** page, add a file, and toggle seeding on. The file's hash is what others need to download it.
- **To download**, go to the **Download** page, paste the file hash, and confirm the payment. The app handles the rest.

## A Tour of the App

| Page | What it does |
|------|---------------|
| **Wallet** | Create or import a wallet, view your recovery phrase, set up email backup. |
| **Network** | See connected peers, network status, and blockchain sync status. |
| **Download** | Search for files by hash and download them, paying in CHI. |
| **Drive** | Organize your local files and choose which ones to share on the network. |
| **ChiralDrop** | Send a file directly to a specific peer, with an optional price. |
| **Hosts** | Hosting marketplace: publish your site, browse CDN servers and peer hosts, manage agreements. |
| **Mining** | Start and stop CPU or GPU mining; track the blocks you've mined. |
| **Account** | Your wallet balance, full transaction history, and reputation panel. |
| **Settings** | Switch between light/dark themes, toggle notifications, set your download folder. |
| **Diagnostics** | Event log and system info — handy if something isn't working. |

## How Payments Work

Chiral Network has its own cryptocurrency called **CHI**. It's used to pay for downloads and hosting, and it's earned by mining or by sharing popular files.

- **Download fee**: 0.01 CHI per megabyte, paid to the file's seeder.
- **Platform fee**: 0.5% of every transaction goes to network upkeep; the rest goes to the seller.
- **Mining reward**: 5 CHI per block mined.
- **Gas fees**: Zero — transactions are free apart from the 0.5% platform fee.

Payments are verified on-chain before any file data is served, so you can't be charged without actually receiving your file, and sellers can't be stiffed after serving.

## Privacy & Security

- **Your wallet lives on your device.** Nobody else — not even the Chiral team — has your recovery phrase. If you lose it, your CHI is gone.
- **File listings are signed.** Every file's metadata and seeder entry is cryptographically signed, so nobody can spoof your files in the network.
- **Payments are verified.** Seeders check the blockchain for your payment before sending chunks.
- **Direct transfers.** When you download a file, it comes directly from the peer (or CDN) sharing it — there's no central middleman storing copies.
- **Update enforcement.** The app checks the network's published version policy on startup and walks you to the download page if your build is below the minimum required version. Vulnerable clients can't quietly stay on the network.

## Running Without a Window (Headless Mode)

If you want to run Chiral Network on a server, a Raspberry Pi, or inside Docker — without a graphical interface — there's a `chiral_daemon` binary that exposes the same features over a local HTTP API. This is mainly useful for always-on seeders, mining rigs, and automated setups. See `CLAUDE.md` for the full API reference.

## Troubleshooting

- **No peers showing up?** Make sure your firewall isn't blocking outbound UDP/TCP traffic. The app connects through a relay server if your network is restrictive.
- **Download stuck?** Check the Diagnostics page for errors. The file may not currently have any seeders online — try the CDN as a fallback.
- **Balance not updating?** Balance is refreshed every few seconds; if it's been longer than a minute, restart the app or check your Geth sync status on the Network page.
- **Lost your wallet?** You can import it again on the Wallet page using your recovery phrase or the email backup.

## License

Proprietary — all rights reserved.
