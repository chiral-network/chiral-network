# IMPORTANT: This document needs full revision. We dont' need overcomplicated wallet and blockchain integration. We only need to support Ethereum-compatible blockchain and a simple wallet.

# Wallet & Blockchain Integration

Chiral Network includes a separate Ethereum-compatible blockchain with HD wallet support and CPU mining capabilities.

## HD Wallet

### Overview

Chiral Network uses Hierarchical Deterministic (HD) wallets based on industry standards:
- **BIP32**: HD wallet structure
- **BIP39**: Mnemonic phrase generation
- **secp256k1**: Elliptic curve cryptography

### Creating a Wallet

#### Generate New Wallet

1. **Navigate to Account Page**
2. **Click "Create Wallet"**
3. **System generates**:
   - 12 or 24-word mnemonic phrase
   - Master private key
   - First account address
4. **Write down mnemonic phrase** (CRITICAL - cannot be recovered)
5. **Verify phrase** by re-entering words
6. **Wallet is created** and ready to use

#### Import Existing Wallet

1. **Navigate to Account Page**
2. **Click "Import Wallet"**
3. **Enter mnemonic phrase**
4. **Optional: Enter derivation path** (default: m/44'/60'/0'/0)
5. **System derives**:
   - Private keys
   - Account addresses
6. **Wallet restored**

### Mnemonic Phrase Security

**Critical Security Rules**:
- ✅ Write down phrase on paper
- ✅ Store in secure location (safe, vault)
- ✅ Never share with anyone
- ✅ Never store digitally (no photos, no cloud)
- ❌ Never enter on websites
- ❌ Never send via email/chat

**Phrase Characteristics**:
- 12 or 24 words from BIP39 wordlist
- Deterministically generates all accounts
- Can restore wallet on any device
- Losing phrase = losing access forever

### Multiple Accounts

HD wallets support multiple accounts:

1. **Derived from single mnemonic**
2. **Each account has unique address**
3. **Derivation path**: m/44'/60'/0'/0/N (N = account index)
4. **Create new accounts** in Account page
5. **Switch between accounts** easily

### Account Management

**Account List Features**:
- View all derived accounts
- See balances for each
- Copy addresses
- Generate QR codes
- Set default account
- Export individual private keys (advanced)

### Wallet Security

**Implemented**:
- Private keys never leave device
- Encrypted storage (if device supports)
- No cloud backup (intentional - security)
- Secure random number generation

**Best Practices**:
- Use strong device password
- Enable disk encryption
- Backup mnemonic phrase securely
- Consider hardware wallet for large amounts

## Blockchain

### Network Details

Chiral Network runs a **separate Ethereum-compatible blockchain**:

- **Network Name**: Chiral Network
- **Chain ID**: Custom (configured in genesis.json)
- **Consensus**: Proof of Work (Ethash)
- **Block Time**: ~15 seconds
- **Gas Limit**: Configurable

### Geth Integration

The application integrates with Geth (Go Ethereum):

**Features**:
- Full Ethereum node
- Transaction signing
- Smart contract deployment
- Block mining
- RPC interface

**Geth Service** (`src/lib/services/gethService.ts`):
- Start/stop Geth node
- Monitor sync status
- Submit transactions
- Query balances

### Proof of Storage Smart Contract

Location: `src/lib/services/ProofOfStorage.sol`

**Purpose**: Validate storage claims through periodic challenges

**Features**:
- Storage commitment registration
- Challenge/response mechanism
- Verification of stored data
- Reward distribution

## Mining

### Overview

Mine blocks to secure the network and earn rewards:

- **Algorithm**: Ethash (Ethereum PoW)
- **Difficulty**: Adjusts based on network hashrate
- **Rewards**: Block reward + transaction fees
- **Hardware**: CPU mining (GPU mining not yet supported)

### Starting Mining

1. **Navigate to Mining Page**
2. **Configure settings**:
   - Number of CPU threads (1-16)
   - Mining intensity (1-100%)
   - Pool selection (solo or pool)
3. **Click "Start Mining"**
4. **Monitor**:
   - Hash rate (H/s, KH/s, MH/s)
   - Blocks found
   - Total rewards
   - Mining history

### Mining Pools

**Note**: Pool UI exists but actual pool mining not yet implemented.

**Available Options**:
- Solo mining (fully functional)
- Pool mining (UI only, coming soon)

### Mining Performance

**Factors Affecting Hashrate**:
- CPU model and speed
- Number of threads
- Mining intensity
- System temperature
- Other running processes

**Optimization Tips**:
- Use all available CPU cores
- Close unnecessary applications
- Ensure adequate cooling
- Monitor temperature
- Adjust intensity if system lags

### Mining Rewards

**Block Rewards**:
- Fixed reward per block (configured in genesis.json)
- Transaction fees (minimal on new network)
- Rewards sent to mining address

**Reward Tracking**:
- Total blocks found
- Total rewards earned
- Recent blocks list
- Mining history chart

**Note**: Reward values in UI may use mock data; actual rewards depend on blockchain configuration.

### Mining History

The Mining page displays:

- **Hash Rate Chart**: Historical hashrate over time
- **Blocks Found**: List of blocks you've mined
- **Power Usage**: Estimated power consumption (mock data)
- **Efficiency**: Hash/watt ratio (mock data)
- **Session Statistics**: Current mining session details

## Transactions

### Sending Transactions

1. **Navigate to Account Page**
2. **Click "Send"**
3. **Enter**:
   - Recipient address
   - Amount (in native token)
   - Gas limit (optional)
   - Gas price (optional)
4. **Review transaction**
5. **Confirm and sign**
6. **Transaction submitted** to blockchain

### Transaction History

View all transactions in Account page:

- **Sent transactions**: Outgoing transfers
- **Received transactions**: Incoming transfers
- **Pending transactions**: Not yet confirmed
- **Failed transactions**: Rejected by network

### Transaction Details

Each transaction shows:
- Transaction hash
- Status (pending/completed/failed)
- Amount
- From/to addresses
- Gas used
- Block number
- Timestamp

## Wallet Features

### QR Codes

Generate QR codes for:
- **Receiving payments**: Share your address
- **Mnemonic backup**: Paper wallet creation
- **Account import**: Easy import on mobile

### Address Book

**Coming Soon**: Save frequently used addresses with labels

### Token Support

**Currently**: Native token only
**Future**: ERC-20 token support

### Hardware Wallet Integration

**Planned Feature**: Support for hardware wallets (Ledger, Trezor)

## Best Practices

### Security

1. **Backup mnemonic phrase** immediately
2. **Use strong passwords** for device encryption
3. **Never share private keys**
4. **Verify addresses** before sending
5. **Test with small amounts** first

### Mining

1. **Monitor temperatures** regularly
2. **Start with lower intensity** and increase gradually
3. **Don't mine on battery** (laptops)
4. **Calculate electricity costs** vs. rewards
5. **Join a pool** for more consistent rewards (when available)

### Transaction Management

1. **Set appropriate gas prices** for urgency
2. **Double-check addresses** before sending
3. **Keep transaction history** for records
4. **Monitor pending transactions**
5. **Contact support** if transaction stuck

## Troubleshooting

### Wallet Issues

**Can't access wallet**:
- Verify mnemonic phrase is correct
- Check derivation path
- Ensure Geth is running
- Restart application

**Balance not showing**:
- Wait for Geth to sync
- Check network connectivity
- Verify correct account selected
- Refresh account page

### Mining Issues

**Mining won't start**:
- Check Geth is running
- Verify mining address is set
- Ensure sufficient system resources
- Check console for errors

**Low hashrate**:
- Increase thread count
- Raise mining intensity
- Close other applications
- Check CPU throttling

**No blocks found**:
- Mining is probabilistic (keep mining)
- Check network difficulty
- Verify connection to peers
- Consider pool mining

### Transaction Issues

**Transaction pending forever**:
- Increase gas price
- Resubmit with higher gas
- Check network congestion
- Verify Geth is synced

**Transaction failed**:
- Check gas limit
- Verify sufficient balance
- Review transaction parameters
- Check smart contract execution

## See Also

- [Security & Privacy](security-privacy.md) - Wallet security details
- [User Guide](user-guide.md) - Step-by-step wallet usage
- [API Documentation](api-documentation.md) - Wallet API reference
