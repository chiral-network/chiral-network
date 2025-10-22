# File Pricing & Upload Payment System

## Purpose:
- In a peer to peer network, file sharing is facilitated by the exchange of Chiral to incentivize downloading and uploading files
- Attach a price to each uploaded file so the network components can enforce payments for downloads or other interactions.
- Ensure the frontend can compute a sensible price, present/confirm it to the user, and pass it to back-end upload/publish flows.
- Provide a clear backend contract so the native/Tauri service and DHT/publish layers can persist and use price metadata.
- Data transmission and payment of files are decoupled and independent



## Overview:
- Currently, Chiral Network implements a BitTorrent-like file sharing model with instant seeding and DHT-based discovery.
- Every uploaded file is associated with a price that is calculated automatically based on its file size. There is no manual updating of the file’s price.
- Prices are calculated on a per-MB rate (0.001 Chiral per MB)


## How it Works:
1. Suppose Node A wants to download ‘file.txt’ that is currently being seeded by Node B.
2. Node A joins the network and establishes connections via handshakes to nearby peer nodes on the network. (DHT Discovery)
3. Node A queries the DHT to find who is seeding this file and the DHT returns a file metadata object that contains the list of the seeders.
4. Node A queries the file hash to find the file and establishes a direct connection with Node B to facilitate the transaction. 
- If Node A has insufficient Chiral to download the file, Node A is gated from being able to download the file. 
- If Node A has sufficient Chiral to download the file, Node A can successfully download the file using either WebRTC or BitSwap Protocols
5. Given Node A has sufficient Chiral to request downloading the file which costs N Chiral, N Chiral will be deducted from Node A’s balance and N Chiral will be added to Node B’s balance
6. Transactions will be logged in both the uploader and downloader’s transaction history



Payment Processing Flow Diagram:
[![image1.png](https://i.postimg.cc/5y3z8zKd/image1.png)](https://postimg.cc/V0J58JcD)

File Upload Process:
[![image2.png](https://i.postimg.cc/Xq8595Ht/image2.png)](https://postimg.cc/K31421y5)

File Discovery & Balance Check:
[![image3.png](https://i.postimg.cc/ZncN6N72/image3.png)](https://postimg.cc/hQXhBXq1)

Download & Payment Process:
[![image4.png](https://i.postimg.cc/PJ4D1DVG/image4.png)](https://postimg.cc/fSVkNVh5)

Seeder Payment Reception:
[![image5.png](https://i.postimg.cc/gjDh3hST/image5.png)](https://postimg.cc/7J5bk5y3)

General File Payment Flow Diagram:
[![image6.png](https://i.postimg.cc/8cHvLvKX/image6.png)](https://postimg.cc/648TX8NC)

High Level Overview of Steps:
[![image7.png](https://i.postimg.cc/kGcSWS11/image7.png)](https://postimg.cc/CnRdgRVk)
[![image8.png](https://i.postimg.cc/gjDh3hSM/image8.png)](https://postimg.cc/bSZdhZjb)
[![image9.png](https://i.postimg.cc/D0gG1Gjx/image9.png)](https://postimg.cc/RJWqBWzt)
[![image10.png](https://i.postimg.cc/SRr9c9TD/image10.png)](https://postimg.cc/BPjt0jGK)


## File Pricing and Metadata:

FileMetadata: Each file has an associated metadata published to the DHT when it is uploaded
```{
    fileHash: string          // SHA-256 content hash
    fileName: string          // Original filename
    fileSize: number          // Size in bytes
    seeders: string[]         // List of seeder peer IDs
    createdAt: number         // Unix timestamp
    merkleRoot?: string       // Merkle tree root for chunks
    mimeType?: string         // File MIME type
    isEncrypted: boolean      // Encryption flag
    encryptionMethod?: string // Encryption algorithm
    keyFingerprint?: string   // Key verification
    version?: number          // File version number
    cids?: string[]           // Content IDs for chunks
    price: number		    // The price in Chiral for the file
    uploader_address: string  // The peer address of the uploader of the file
}
```
- Price and uploader_address fields are added to the FileMetadata struct
- Given the fileSize in bytes, files prices are automatically calculated based on price-per-MB settings (0.001 Chiral per MB)
- Uploader’s wallet address contains additional metadata for accurate and efficient payment routing

## Features:

### File Prices:
- Price computation for files happens automatically after they are uploaded. Using the fileSize field which represents the amount of Bytes the file takes up, we convert this to MB to determine the amount of Chiral this file costs at the conversion rate of 0.001 Chiral per MB
- File prices should persist in the backend. When a seeder uploads a file, the price of the file should be maintained and remain the same across multiple sessions
- Secure transaction processing with Ethereum integration



### Payment Service (Backend):
- Be able to fetch the current user’s wallet balance and update the balance as transactions are made
- Ensure that the current user has enough Chiral before any transactions can occur
- Execute and facilitate payment transaction across two connected peers ensuring that accurate additions and removals are made to each users’ wallet
- Log payment details in the current user’s wallet and transaction history
- Handle seeder and downloader payment receipts



### Payment Service (Frontend):
- Automatic payment calculation based on file size
- Price Badges showing cost of files in Chiral tokens
- Enhanced File cards showing transaction-related information
- Balance checks and validations before downloads
- Payment confirmation modals
- Transaction changes recording and history in Account page
- Live P2P Payment toasts and notifications between peers for successful and failed transactions and downloads

## Areas for Improvement and TODOs:
- Define clear rounding rules, to which decimal place do we show up to and do we round?
- Adding unit and integration tests for price calculation and persistence
- Handle edge cases such as for empty files
- Audit logs: record who uploaded, price passed, timestamp and any changes to price for compliance.
- Implement timeouts, transaction timeouts or invoices if process takes too long
- Gas prices to be added to cost as a fee for initiating a transaction
- File payments, upload and download not working correctly for Windows-Windows sharing
- Implementation and documentation for when there are multiple seeders for the same file and handling payments for this scenario.
- More specific details on price/payment specification (apis, message formats and etc.)


