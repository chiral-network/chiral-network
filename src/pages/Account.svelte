<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { Wallet, Copy, ArrowUpRight, ArrowDownLeft, History, Coins, Plus, Import, BadgeX } from 'lucide-svelte'
  import { wallet, etcAccount, blacklist } from '$lib/stores'
  import { writable, derived } from 'svelte/store'
  import { invoke } from '@tauri-apps/api/core'

  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

  interface Transaction {
    id: number;
    type: 'sent' | 'received';
    amount: number;
    to?: string;
    from?: string;
    date: Date;
    description: string;
    status: 'pending' | 'completed';
  }
  
  let recipientAddress = ''
  let sendAmount = 0
  let rawAmountInput = '' // Track raw user input for validation
  let privateKeyVisible = false
  let showPending = false
  let importPrivateKey = ''
  let isCreatingAccount = false
  let isImportingAccount = false
  let isGethRunning = false
  
  // Demo transactions - in real app these will be fetched from blockchain
  const transactions = writable<Transaction[]>([
    { id: 1, type: 'received', amount: 50.5, from: '0x8765...4321', to: undefined, date: new Date('2024-03-15'), description: 'File purchase', status: 'completed' },
    { id: 2, type: 'sent', amount: 10.25, to: '0x1234...5678', from: undefined, date: new Date('2024-03-14'), description: 'Proxy service', status: 'completed' },
    { id: 3, type: 'received', amount: 100, from: '0xabcd...ef12', to: undefined, date: new Date('2024-03-13'), description: 'Upload reward', status: 'completed' },
    { id: 4, type: 'sent', amount: 5.5, to: '0x9876...5432', from: undefined, date: new Date('2024-03-12'), description: 'File download', status: 'completed' },
  ]);

  // Enhanced validation states
  let validationWarning = '';
  let isAmountValid = true;
  let addressWarning = '';
  let isAddressValid = false;


   // Copy feedback message
   let copyMessage = '';
   let privateKeyCopyMessage = '';
   
   // Export feedback message
   let exportMessage = '';
  // Filtering state
  let filterType: 'all' | 'sent' | 'received' = 'all';
  let filterDateFrom: string = '';
  let filterDateTo: string = '';
  let sortDescending: boolean = true;
  
  // Fetch balance when account changes
  $: if ($etcAccount && isGethRunning) {
    fetchBalance()
  }

  // Derived filtered transactions
  $: filteredTransactions = $transactions
    .filter(tx => {
      const matchesType = filterType === 'all' || tx.type === filterType;
      const txDate = tx.date instanceof Date ? tx.date : new Date(tx.date);
      const fromOk = !filterDateFrom || txDate >= new Date(filterDateFrom + 'T00:00:00'); // the start of day
      const toOk = !filterDateTo || txDate <= new Date(filterDateTo + 'T23:59:59'); // and the end of day to include full date ranges
      return matchesType && fromOk && toOk;
    })
    .slice()
    .sort((a, b) => {
      const dateA = a.date instanceof Date ? a.date : new Date(a.date);
      const dateB = b.date instanceof Date ? b.date : new Date(b.date);
      return sortDescending ? dateB.getTime() - dateA.getTime() : dateA.getTime() - dateB.getTime();
    });

  // Enhanced address validation with immediate feedback
  $: {
    if (recipientAddress === '') {
      addressWarning = '';
      isAddressValid = false;
    } else if (!recipientAddress.startsWith('0x')) {
      addressWarning = 'Address must start with 0x';
      isAddressValid = false;
    } else if (recipientAddress.length > 42) {
      addressWarning = 'Address is too long (max 42 characters)';
      isAddressValid = false;
    }
    else if (!isValidHexAddress(recipientAddress)) {
      addressWarning = 'Address must contain valid hexadecimal characters (0-9, a-f, A-F)';
      isAddressValid = false;
    }
    else if (recipientAddress.length < 42) {
      addressWarning = 'Address must be exactly 42 characters long';
      isAddressValid = false;
    }
    else {
      addressWarning = '';
      isAddressValid = true;
    }
  }

  // Amount validation
  $: {
    if (rawAmountInput === '') {
      validationWarning = '';
      isAmountValid = false;
      sendAmount = 0;
    } else {
      const inputValue = parseFloat(rawAmountInput);

      if (isNaN(inputValue) || inputValue <= 0) {
        validationWarning = 'Please enter a valid amount greater than 0.';
        isAmountValid = false;
        sendAmount = 0;
      } else if (inputValue < 0.01) {
        validationWarning = `Amount must be at least 0.01 CN.`;
        isAmountValid = false;
        sendAmount = 0;
      } else if (inputValue > $wallet.balance) {
        validationWarning = `Insufficient balance - Need ${(inputValue - $wallet.balance).toFixed(2)} more CN.`;
        isAmountValid = false;
        sendAmount = 0;
      } else {
        // Valid amount
        validationWarning = '';
        isAmountValid = true;
        sendAmount = inputValue;
      }
    }
  }

  // Enhanced address validation function
  function isValidHexAddress(address: string): boolean {
    // Check that everything after 0x is hexadecimal
    const hexPart = address.slice(2);
    if (hexPart.length === 0) return false;

    const hexRegex = /^[a-fA-F0-9]+$/;
    return hexRegex.test(hexPart);
  }
  
  function copyAddress() {
    const addressToCopy = $etcAccount ? $etcAccount.address : $wallet.address;
    navigator.clipboard.writeText(addressToCopy);
    copyMessage = 'Copied!';
    setTimeout(() => copyMessage = '', 1500);
  }

  function copyPrivateKey() {
    navigator.clipboard.writeText('your-private-key-here-do-not-share');
    privateKeyCopyMessage = 'Copied!';
    setTimeout(() => privateKeyCopyMessage = '', 1500);
  }
  
  
  async function exportWallet() {
    try {
      throw new Error('Test error message');
      const walletData = {
        address: $wallet.address,
        privateKey: "your-private-key-here-do-not-share", // this should change to be the actual private key
        balance: $wallet.balance,
        totalEarned: $wallet.totalEarned,
        totalSpent: $wallet.totalSpent,
        pendingTransactions: $wallet.pendingTransactions,
        exportDate: new Date().toISOString(),
        version: "1.0"
      };
      
      const dataStr = JSON.stringify(walletData, null, 2);
      const dataBlob = new Blob([dataStr], { type: 'application/json' });
      
      // Check if the File System Access API is supported
      if ('showSaveFilePicker' in window) {
        try {
          const fileHandle = await (window as any).showSaveFilePicker({
            suggestedName: `chiral-wallet-export-${new Date().toISOString().split('T')[0]}.json`,
            types: [{
              description: 'JSON files',
              accept: {
                'application/json': ['.json'],
              },
            }],
          });
          
          const writable = await fileHandle.createWritable();
          await writable.write(dataBlob);
          await writable.close();
          
          exportMessage = 'Wallet exported successfully!';
        } catch (error: any) {
          if (error.name !== 'AbortError') {
            throw error;
          }
          // User cancelled, don't show error message
          return;
        }
      } else {
        // Fallback for browsers that don't support File System Access API
        const url = URL.createObjectURL(dataBlob);
        const link = document.createElement('a');
        link.href = url;
        link.download = `chiral-wallet-export-${new Date().toISOString().split('T')[0]}.json`;
        document.body.appendChild(link);
        link.click();
        document.body.removeChild(link);
        URL.revokeObjectURL(url);
        
        exportMessage = 'Wallet exported successfully!';
      }
      
      setTimeout(() => exportMessage = '', 3000);
    } catch (error) {
      console.error('Export failed:', error);
      exportMessage = 'Export failed. Please try again.';
      setTimeout(() => exportMessage = '', 3000);
    }
  }
  
  function sendTransaction() {
    if (!isAddressValid || !isAmountValid || !isAddressValid || sendAmount <= 0) return
    
    // Simulate transaction
    wallet.update(w => ({
      ...w,
      balance: w.balance - sendAmount,
      pendingTransactions: w.pendingTransactions + 1,
      totalSpent: w.totalSpent + sendAmount
    }))

    transactions.update(txs => [
    {
      id: Date.now(),
      type: 'sent',
      amount: sendAmount,
      to: recipientAddress,
      date: new Date(),
      description: 'Manual transaction',
      status: 'pending'
    },
    ...txs // prepend so latest is first
  ])
    
    // Clear form
    recipientAddress = ''
    sendAmount = 0
    rawAmountInput = ''
    
    // Simulate transaction completion
    setTimeout(() => {
      wallet.update(w => ({
        ...w,
        pendingTransactions: Math.max(0, w.pendingTransactions - 1)
      }))
      transactions.update(txs => txs.map(tx => tx.status === 'pending' ? { ...tx, status: 'completed' } : tx))
    }, 3000)
  }
  
  function formatDate(date: Date): string {
    return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' })
  }

  // Ensure wallet.pendingTransactions matches actual pending transactions
  const pendingCount = derived(transactions, $txs => $txs.filter(tx => tx.status === 'pending').length);

  // Ensure pendingCount is used (for linter)
  $: void $pendingCount;

  import { onMount } from 'svelte'
  
  let balanceInterval: number | undefined
  
  onMount(() => {
    checkGethStatus()

    // Set up periodic balance refresh every 10 seconds
    balanceInterval = window.setInterval(() => {
      if ($etcAccount && isGethRunning) {
        fetchBalance()
      }
    }, 10000)

    // Cleanup function
    return () => {
      if (balanceInterval) window.clearInterval(balanceInterval)
    }
  })

  async function checkGethStatus() {
    try {
      if (isTauri) {
        isGethRunning = await invoke('is_geth_running') as boolean
        // Fetch balance if account exists and geth is running
        if ($etcAccount && isGethRunning) {
          fetchBalance()
        }
      } else {
        // Fallback for web environment - assume geth is not running
        isGethRunning = false
        console.log('Running in web mode - geth not available')
      }
    } catch (error) {
      console.error('Failed to check geth status:', error)
    }
  }

  async function fetchBalance() {
    if (!$etcAccount) return
    
    try {
      if (isTauri && isGethRunning) {
        // Desktop app with local geth node - get real blockchain balance
        const balance = await invoke('get_account_balance', { address: $etcAccount.address }) as string
        wallet.update(w => ({ ...w, balance: parseFloat(balance) }))
      } else if (isTauri && !isGethRunning) {
        // Desktop app but geth not running - use stored balance
        console.log('Geth not running - using stored balance')
      } else {
        // Web environment - For now, simulate balance updates for demo purposes
        const simulatedBalance = $wallet.balance + Math.random() * 10 // Small random changes
        wallet.update(w => ({ ...w, balance: Math.max(0, simulatedBalance) }))
      }
    } catch (error) {
      console.error('Failed to fetch balance:', error)
      // Fallback to stored balance on error
    }
  }

  async function createChiralAccount() {
    isCreatingAccount = true
    try {

      let account: { address: string, private_key: string, blacklist: Object[] }

      if (isTauri) {
        // Use Tauri backend
        account = await invoke('create_chiral_account') as { address: string, private_key: string, blacklist: Object[] }
      } else {
        // Fallback for web environment - generate demo account
        const demoAddress = '0x' + Math.random().toString(16).substr(2, 40)
        const demoPrivateKey = '0x' + Math.random().toString(16).substr(2, 64)
        const demoBlackList = [{node_id: 169245, name: "Jane"}]
        account = {
          address: demoAddress,
          private_key: demoPrivateKey,
          blacklist: demoBlackList
        }
        console.log('Running in web mode - using demo account')
      }
      
      // Update the Chiral account store
      etcAccount.set(account)
      // Also update the wallet store with the new Chiral address
      wallet.update(w => ({
        ...w,
        address: account.address
      }))
      // Private key stays hidden by default
      
      // Fetch balance for new account
      if (isGethRunning) {
        await fetchBalance()
      }
    } catch (error) {
      console.error('Failed to create Chiral account:', error)
      alert('Failed to create account: ' + error)
    } finally {
      isCreatingAccount = false
    }
  }

  async function importChiralAccount() {
    if (!importPrivateKey) return
    
    isImportingAccount = true
    try {
      let account: { address: string, private_key: string }
      
      if (isTauri) {
        // Use Tauri backend
        account = await invoke('import_chiral_account', { privateKey: importPrivateKey }) as { address: string, private_key: string }
      } else {
        // Fallback for web environment - use the provided private key
        // In a real implementation, you'd derive the address from the private key
        const demoAddress = '0x' + Math.random().toString(16).substr(2, 40)
        account = {
          address: demoAddress,
          private_key: importPrivateKey
        }
        console.log('Running in web mode - using provided private key')
      }
      
      // Update the Chiral account store
      etcAccount.set(account)
      // Also update the wallet store with the imported Chiral address
      wallet.update(w => ({
        ...w,
        address: account.address
      }))
      importPrivateKey = ''
      // Private key stays hidden by default
      
      // Fetch balance for imported account
      if (isGethRunning) {
        await fetchBalance()
      }
    } catch (error) {
      console.error('Failed to import Chiral account:', error)
      alert('Failed to import account: ' + error)
    } finally {
      isImportingAccount = false
    }
  }

  let newBlacklistEntry = {
    chiral_address: "",
    description: ""
  }

  $: isBlacklistFormValid = 
    newBlacklistEntry.description.trim() !== '' &&
    newBlacklistEntry.chiral_address.startsWith('0x') &&
    newBlacklistEntry.chiral_address.length === 42;
  
  function addBlacklistEntry() {
    if (newBlacklistEntry.chiral_address && newBlacklistEntry.description) {
      blacklist.update(entries => [...entries,
        { chiral_address: newBlacklistEntry.chiral_address, reason: newBlacklistEntry.description, timestamp: new Date() }
      ]);
      newBlacklistEntry = { chiral_address: "", description: "" }; // Clear input fields
    }
  }


  function removeBlacklistEntry(chiral_address: string) {
    blacklist.update(entries => {
      return entries.filter(entry => entry.chiral_address !== chiral_address);
    });
  }

  
  // Helper function to set max amount
  function setMaxAmount() {
    rawAmountInput = $wallet.balance.toString();
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Account</h1>
    <p class="text-muted-foreground mt-2">Manage your wallet and account settings</p>
  </div>
  
  <div class="grid grid-cols-1 {$etcAccount ? 'md:grid-cols-2' : ''} gap-4">
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">Chiral Network Wallet</h2>
        <Wallet class="h-5 w-5 text-muted-foreground" />
      </div>
      
      <div class="space-y-4">
        {#if !$etcAccount}
          <div class="space-y-3">
            <p class="text-sm text-muted-foreground">Get started with Chiral Network by creating or importing an account:</p>
            
            
            <Button 
              class="w-full" 
              on:click={createChiralAccount}
              disabled={isCreatingAccount}
            >
              <Plus class="h-4 w-4 mr-2" />
              {isCreatingAccount ? 'Creating...' : 'Create New Account'}
            </Button>
            
            <div class="space-y-2">
              <Input
                type="text"
                bind:value={importPrivateKey}
                placeholder="Enter private key to import"
                class="w-full"
                autocomplete="off"
                data-form-type="other"
                data-lpignore="true"
                spellcheck="false"
              />
              <Button 
                class="w-full" 
                variant="outline"
                on:click={importChiralAccount}
                disabled={!importPrivateKey || isImportingAccount}
              >
                <Import class="h-4 w-4 mr-2" />
                {isImportingAccount ? 'Importing...' : 'Import Existing Account'}
              </Button>
            </div>
          </div>
        {:else}
          <div>
            <!-- Balance Display - Only when logged in -->
        <div>
          <p class="text-sm text-muted-foreground">Balance</p>
          <p class="text-2xl font-bold">{$wallet.balance.toFixed(2)} Chiral</p>
        </div>
        
            <div class="grid grid-cols-2 gap-4 mt-4">
          <div>
            <p class="text-xs text-muted-foreground">Total Earned</p>
            <p class="text-sm font-medium text-green-600">+{$wallet.totalEarned.toFixed(2)} Chiral</p>
          </div>
          <div>
            <p class="text-xs text-muted-foreground">Total Spent</p>
            <p class="text-sm font-medium text-red-600">-{$wallet.totalSpent.toFixed(2)} Chiral</p>
          </div>
        </div>
        
            <div class="mt-6">
              <p class="text-sm text-muted-foreground">Chiral Address</p>
              <div class="flex items-center gap-2 mt-1">
                <p class="font-mono text-sm">{$etcAccount.address.slice(0, 10)}...{$etcAccount.address.slice(-8)}</p>
                <div class="relative">
                  <Button size="sm" variant="outline" on:click={copyAddress}>
                    <Copy class="h-3 w-3" />
                  </Button>
                  {#if copyMessage}
                    <span class="absolute top-full left-1/2 transform -translate-x-1/2 text-xs text-green-600 mt-1 whitespace-nowrap">{copyMessage}</span>
                  {/if}
                </div>
              </div>
            </div>
            
            <div class="mt-4">
              <p class="text-sm text-muted-foreground">Private Key</p>
                <div class="flex gap-2 mt-1">
                  <Input
                    type="text"
                    value={privateKeyVisible ? $etcAccount.private_key : '•'.repeat($etcAccount.private_key.length)}
                    readonly
                    class="flex-1 font-mono text-xs min-w-0"
                  />
                <div class="relative">
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={copyPrivateKey}
                  >
                    <Copy class="h-3 w-3" />
                  </Button>
                  {#if privateKeyCopyMessage}
                    <span class="absolute top-full left-1/2 transform -translate-x-1/2 text-xs text-green-600 mt-1 whitespace-nowrap">{privateKeyCopyMessage}</span>
                  {/if}
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  class="w-16"
                  on:click={() => privateKeyVisible = !privateKeyVisible}
                >
                  {privateKeyVisible ? 'Hide' : 'Show'}
                </Button>
              </div>
               <p class="text-xs text-muted-foreground mt-1">Never share your private key with anyone</p>
             </div>
             
             <div class="mt-4">
               <Button type="button" variant="outline" class="w-full" on:click={exportWallet}>
                 Export Wallet
               </Button>
               {#if exportMessage}
                 <p class="text-xs text-center mt-2 {exportMessage.includes('successfully') ? 'text-green-600' : 'text-red-600'}">{exportMessage}</p>
               {/if}
             </div>
           </div>
         {/if}
      </div>
    </Card>
    
    {#if $etcAccount}
    <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Send Chiral Coins</h2>
      <Coins class="h-5 w-5 text-muted-foreground" />
    </div>
    <form autocomplete="off" data-form-type="other" data-lpignore="true">
      <div class="space-y-4">
        <div>
          <Label for="recipient">Recipient Address</Label>
          <Input
            id="recipient"
            bind:value={recipientAddress}
            placeholder="0x..."
            class="mt-2 {addressWarning ? 'border-red-500' : ''}"
            data-form-type="other"
            data-lpignore="true"
            aria-autocomplete="none"
          />
          <div class="flex items-center justify-between mt-1">
            <span class="text-xs text-muted-foreground">
              {recipientAddress.length}/42 characters ({42 - recipientAddress.length} remaining)
            </span>
            {#if addressWarning}
              <p class="text-xs text-red-500 font-medium">{addressWarning}</p>
            {/if}
          </div>
        </div>

        <div>
          <Label for="amount">Amount (Chiral)</Label>
          <div class="relative mt-2">
            <Input
              id="amount"
              type="number"
              bind:value={rawAmountInput}
              placeholder=""
              min="0.01"
              step="0.01"
              class="mt-2 [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none [-moz-appearance:textfield]"
              data-form-type="other"
              data-lpignore="true"
              aria-autocomplete="none"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              class="absolute right-1 top-1/2 transform -translate-y-1/2 h-8 px-3"
              on:click={setMaxAmount}
              disabled={$wallet.balance <= 0}
            >
              Max
            </Button>
          </div>
          <div class="flex items-center justify-between mt-1">
            <p class="text-xs text-muted-foreground">
              Available: {$wallet.balance.toFixed(2)} Chiral
            </p>
            {#if validationWarning}
              <p class="text-xs text-red-500 font-medium">{validationWarning}</p>
            {/if}
          </div>
        
        </div>

        <Button
          type="button"
          class="w-full"
          on:click={sendTransaction}
          disabled={!isAddressValid || !isAmountValid || !isAddressValid || rawAmountInput === ''}
        >
          <ArrowUpRight class="h-4 w-4 mr-2" />
          Send Transaction
        </Button>

        <Button type="button" class="w-full justify-center bg-gray-100 hover:bg-gray-200 text-gray-800 rounded transition-colors py-2 font-normal" on:click={() => showPending = !showPending} aria-label="View pending transactions">
          <span class="flex items-center gap-2">
            <svg class="h-4 w-4 text-orange-500" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24">
              <circle cx="12" cy="10" r="8" />
              <polyline points="12,6 12,10 16,14" />
            </svg>
            {#if $pendingCount > 0}
              {$pendingCount} Pending Transaction{$pendingCount !== 1 ? 's' : ''}
            {:else}
              Pending Transactions
            {/if}
          </span>
        </Button>
        {#if showPending}
          <div class="mt-2 p-3 bg-gray-50 rounded shadow">
            <h3 class="text-sm mb-2 text-gray-700 font-normal">Pending Transactions</h3>
            <ul class="space-y-1">
              {#each $transactions.filter(tx => tx.status === 'pending') as tx}
                <li class="text-xs text-gray-800 font-normal">
                  {tx.description} ({tx.type === 'sent' ? 'To' : 'From'}: {tx.type === 'sent' ? tx.to : tx.from}) - {tx.amount} Chiral
                </li>
              {:else}
                <li class="text-xs text-gray-500 font-normal">No pending transaction details available.</li>
              {/each}
            </ul>
          </div>
        {/if}
        </div>
      </form>
    </Card>
  {/if}
  </div>
  
  {#if $etcAccount}
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Transaction History</h2>
      <History class="h-5 w-5 text-muted-foreground" />
    </div>
    <!-- Filter Controls -->
    <div class="flex flex-wrap gap-4 mb-4 items-end">
      <div>
        <label for="filter-type" class="block text-xs font-medium mb-1">Type</label>
        <select id="filter-type" bind:value={filterType} class="border rounded px-2 py-1 text-sm">
          <option value="all">All</option>
          <option value="sent">Sent</option>
          <option value="received">Received</option>
        </select>
      </div>
      <div>
        <label for="filter-date-from" class="block text-xs font-medium mb-1">From</label>
        <input id="filter-date-from" type="date" bind:value={filterDateFrom} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label for="filter-date-to" class="block text-xs font-medium mb-1">To</label>
        <input id="filter-date-to" type="date" bind:value={filterDateTo} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label for="sort-button" class="block text-xs font-medium mb-1">Sort</label>
        <button id="sort-button" type="button" class="border rounded px-3 py-1 text-sm bg-white hover:bg-gray-100 transition-colors w-full" on:click={() => { sortDescending = !sortDescending; }} aria-pressed={sortDescending}>
          {sortDescending ? 'Newest → Oldest' : 'Oldest → Newest'}
        </button>
      </div>
      <div class="flex-1"></div>
      <div class="flex flex-col gap-1 items-end">
        <button type="button" class="border rounded px-3 py-1 text-sm bg-muted hover:bg-muted/70 transition-colors" on:click={() => { filterType = 'all'; filterDateFrom = ''; filterDateTo = ''; sortDescending = true; }}>
          Reset
        </button>
      </div>
    </div>
    <div class="space-y-2 max-h-80 overflow-y-auto pr-1">
      {#each filteredTransactions as tx}
        <div class="flex items-center justify-between p-3 bg-secondary rounded-lg">
          <div class="flex items-center gap-3">
            {#if tx.type === 'received'}
              <ArrowDownLeft class="h-4 w-4 text-green-500" />
            {:else}
              <ArrowUpRight class="h-4 w-4 text-red-500" />
            {/if}
            <div>
              <p class="text-sm font-medium">{tx.description}</p>
              <p class="text-xs text-muted-foreground">
                {tx.type === 'received' ? 'From' : 'To'}: {tx.type === 'received' ? tx.from : tx.to}
              </p>
            </div>
          </div>
          <div class="text-right">
            <p class="text-sm font-medium {tx.type === 'received' ? 'text-green-600' : 'text-red-600'}">
              {tx.type === 'received' ? '+' : '-'}{tx.amount} Chiral
            </p>
            <p class="text-xs text-muted-foreground">{formatDate(tx.date)}</p>
          </div>
        </div>
      {/each}
      {#if filteredTransactions.length === 0}
        <div class="text-center py-8 text-muted-foreground">
          <History class="h-12 w-12 mx-auto mb-2 opacity-20" />
          <p>No transactions yet</p>
          <p class="text-sm mt-1">Transactions will appear here once you send or receive Chiral</p>
        </div>
      {/if}
    </div>
  </Card>

  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Blacklist Management</h2>
      <BadgeX class="h-5 w-5 text-muted-foreground" />
    </div>


    <div class="space-y-4">
      <div>
        <Label for="blacklist-address">Chrial Address to Blacklist</Label>
        <Input
          id="blacklist-address"
          bind:value={newBlacklistEntry.chiral_address}
          placeholder="e.g., 0x1234...5678 (42 characters)"
          class="mt-2"
        />
      </div>
      <div>
        <Label for="blacklist-name">Reason</Label>
        <Input
          id="blacklist-name"
          bind:value={newBlacklistEntry.description}
          placeholder="e.g., Malicious Peer"
          class="mt-2"
        />
      </div>
      <Button type="button" class="w-full" disabled={!isBlacklistFormValid} on:click={addBlacklistEntry}>
        Add to Blacklist
      </Button>


      <h3 class="text-md font-semibold mt-6 mb-3">Blacklisted Addresses</h3>
      {#if $blacklist.length === 0}
        <p class="text-sm text-muted-foreground">No addresses blacklisted yet.</p>
      {:else}
        <div class="space-y-2">
          {#each $blacklist as entry (entry.chiral_address)}
            <div class="flex items-center justify-between p-3 bg-secondary rounded-lg">
              <div>
                <p class="text-sm font-medium">{entry.chiral_address}</p>
                <p class="text-xs text-muted-foreground">{entry.reason}</p>
              </div>
              <Button size="sm" variant="destructive" on:click={() => removeBlacklistEntry(entry.chiral_address)}>
                Remove
              </Button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </Card>
  {/if}

</div>
