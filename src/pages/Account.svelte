<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { Wallet, Copy, ArrowUpRight, ArrowDownLeft, History, Coins, Plus, Import } from 'lucide-svelte'
  import { wallet, etcAccount } from '$lib/stores'
  import { writable, derived } from 'svelte/store'
  import { invoke } from '@tauri-apps/api/core'

  let showClearHistoryModal = false;
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

  // Validation states
  let validationWarning = '';
  let isAmountValid = true;

  // Copy feedback message
  let copyMessage = '';
  let privateKeyCopyMessage = '';
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
      const fromOk = !filterDateFrom || txDate >= new Date(filterDateFrom);
      const toOk = !filterDateTo || txDate <= new Date(filterDateTo);
      return matchesType && fromOk && toOk;
    })
    .slice()
    .sort((a, b) => {
      const dateA = a.date instanceof Date ? a.date : new Date(a.date);
      const dateB = b.date instanceof Date ? b.date : new Date(b.date);
      return sortDescending ? dateB.getTime() - dateA.getTime() : dateA.getTime() - dateB.getTime();
    });

  // Validation logic
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

  function handleConfirmClear(){
    transactions.set([]);
    showClearHistoryModal = false;
  }
  
  function sendTransaction() {
    if (!recipientAddress || !isAmountValid || sendAmount <= 0) return

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
      let account: { address: string, private_key: string }
      
      if (isTauri) {
        // Use Tauri backend
        account = await invoke('create_chiral_account') as { address: string, private_key: string }
      } else {
        // Fallback for web environment - generate demo account
        const demoAddress = '0x' + Math.random().toString(16).substr(2, 40)
        const demoPrivateKey = '0x' + Math.random().toString(16).substr(2, 64)
        account = {
          address: demoAddress,
          private_key: demoPrivateKey
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
              <p class="text-2xl font-bold">{$wallet.balance.toFixed(2)} CN</p>
            </div>
            
            <div class="grid grid-cols-2 gap-4 mt-4">
              <div>
                <p class="text-xs text-muted-foreground">Total Earned</p>
                <p class="text-sm font-medium text-green-600">+{$wallet.totalEarned.toFixed(2)} CN</p>
              </div>
              <div>
                <p class="text-xs text-muted-foreground">Total Spent</p>
                <p class="text-sm font-medium text-red-600">-{$wallet.totalSpent.toFixed(2)} CN</p>
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
          </div>
        {/if}
      </div>
    </Card>
    
    {#if $etcAccount}
    <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Send CN Tokens</h2>
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
            class="mt-2"
            data-form-type="other"
            data-lpignore="true"
            aria-autocomplete="none"
          />
        </div>

        <div>
          <Label for="amount">Amount (CN)</Label>
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
          <div class="flex items-center justify-between mt-1">
            <p class="text-xs text-muted-foreground">
              Available: {$wallet.balance.toFixed(2)} CN
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
          disabled={!recipientAddress || !isAmountValid || rawAmountInput === ''}
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
                  {tx.description} ({tx.type === 'sent' ? 'To' : 'From'}: {tx.type === 'sent' ? tx.to : tx.from}) - {tx.amount} CN
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
        <label for="filter-from" class="block text-xs font-medium mb-1">From</label>
        <input id="filter-from" type="date" bind:value={filterDateFrom} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label for="filter-to" class="block text-xs font-medium mb-1">To</label>
        <input id="filter-to" type="date" bind:value={filterDateTo} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label for="sort-button" class="block text-xs font-medium mb-1">Sort</label>
        <button id="sort-button" type="button" class="border rounded px-3 py-1 text-sm bg-white hover:bg-gray-100 transition-colors w-full" on:click={() => { sortDescending = !sortDescending; }}>
          {sortDescending ? 'Newest → Oldest' : 'Oldest → Newest'}
        </button>
      </div>
      <div class="flex-1"></div>
      <div class="flex flex-row gap-1 items-end">
        <button type="button" class="border rounded px-3 py-1 text-sm bg-muted hover:bg-muted/70 transition-colors" on:click={() => { filterType = 'all'; filterDateFrom = ''; filterDateTo = ''; sortDescending = true; }}>
          Reset
        </button>
        <button type="button" class="border rounded px-3 py-1 text-sm bg-red-50 text-red-700 hover:bg-red-100 transition-colors" on:click={() => showClearHistoryModal = true}>
          Clear History
        </button>
        {#if showClearHistoryModal}
        <div
          class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
          role="button"
          tabindex="0"
          on:click={() => showClearHistoryModal = false}
          on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') showClearHistoryModal = false; }}
        >
          <div class="bg-white p-6 rounded-lg shadow-xl w-full max-w-sm" role="dialog" aria-modal="true">
            <h3 class="text-lg font-bold mb-2">Are you sure?</h3>
            <p class="text-sm text-gray-600 mb-6">
              All transaction history will be permanently deleted. This action cannot be undone.
            </p>
            <div class="flex justify-end gap-3">
              <button type="button"  class="px-4 py-2 rounded border" on:click={() => showClearHistoryModal = false}>
                Cancel
              </button>
              <button type="button"  class="px-4 py-2 rounded border bg-red-600 text-white" on:click={handleConfirmClear}>
                Confirm Clear
              </button>
            </div>
          </div>
        </div>
      {/if}
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
              {tx.type === 'received' ? '+' : '-'}{tx.amount} CN
            </p>
            <p class="text-xs text-muted-foreground">{formatDate(tx.date)}</p>
          </div>
        </div>
      {:else}
        <div class="text-center py-8 text-muted-foreground">
          <History class="h-12 w-12 mx-auto mb-2 opacity-20" />
          <p>No transactions yet</p>
          <p class="text-sm mt-1">Transactions will appear here once you send or receive CN</p>
        </div>
      {/each}
    </div>
  </Card>
  {/if}
</div>