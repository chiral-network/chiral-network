<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { Wallet, Copy, ArrowUpRight, ArrowDownLeft, Settings, Key, History, Coins, Plus, Import } from 'lucide-svelte'
  import { wallet, etcAccount } from '$lib/stores'
  import { writable, derived } from 'svelte/store'
  import { invoke } from '@tauri-apps/api/core'
  
  let recipientAddress = ''
  let sendAmount = 0
  let privateKeyVisible = false
  let showPending = false
  let importPrivateKey = ''
  let isCreatingAccount = false
  let isImportingAccount = false
  let password = ''
  let confirmPassword = ''
  let unlockPassword = ''
  let savedAccounts: string[] = []
  let selectedSavedAccount = ''
  let showPasswordModal = false
  let passwordAction: 'create' | 'import' | 'unlock' = 'create'
  let accountBalance = '0.000000'
  let isLoadingBalance = false
  let isGethRunning = false
  
  // Real transactions will be fetched from blockchain in the future
  const transactions = writable([]);

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

  // Warning message for amount input
  let amountWarning = ''

  // Copy feedback message
  let copyMessage = ''

  $: {
    const maxAmount = parseFloat(accountBalance) || 0
    const prevAmount = sendAmount
    sendAmount = Math.max(0.000001, Math.min(sendAmount, maxAmount))
    amountWarning = (prevAmount !== sendAmount)
      ? `Amount cannot be ${prevAmount}. Allowed range: 0.000001-${accountBalance} CN.`
      : ''
  }
  
  function copyAddress() {
    const addressToCopy = $etcAccount ? $etcAccount.address : $wallet.address;
    navigator.clipboard.writeText(addressToCopy);
    copyMessage = 'Copied!';
    setTimeout(() => copyMessage = '', 1500);
  }
  
  function sendTransaction() {
    if (!recipientAddress || sendAmount <= 0) return
    
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
    
    recipientAddress = ''
    sendAmount = 0
    
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

  // Load saved accounts on mount
  import { onMount } from 'svelte'
  
  let balanceInterval: number | undefined
  
  onMount(async () => {
    await loadSavedAccounts()
    await checkGethStatus()
    
    // Set up periodic balance refresh every 10 seconds
    balanceInterval = setInterval(() => {
      if ($etcAccount && isGethRunning) {
        fetchBalance()
      }
    }, 10000)
    
    return () => {
      if (balanceInterval) clearInterval(balanceInterval)
    }
  })

  async function loadSavedAccounts() {
    try {
      savedAccounts = await invoke('list_keystore_accounts') as string[]
    } catch (error) {
      console.error('Failed to load saved accounts:', error)
    }
  }

  async function checkGethStatus() {
    try {
      isGethRunning = await invoke('is_geth_running') as boolean
      // Fetch balance if account exists and geth is running
      if ($etcAccount && isGethRunning) {
        fetchBalance()
      }
    } catch (error) {
      console.error('Failed to check geth status:', error)
    }
  }

  async function fetchBalance() {
    if (!$etcAccount || !isGethRunning) return
    
    isLoadingBalance = true
    try {
      accountBalance = await invoke('get_account_balance', { address: $etcAccount.address }) as string
    } catch (error) {
      console.error('Failed to fetch balance:', error)
      accountBalance = '0.000000'
    } finally {
      isLoadingBalance = false
    }
  }


  async function createChiralAccount() {
    passwordAction = 'create'
    showPasswordModal = true
  }

  async function confirmCreateAccount() {
    if (password !== confirmPassword) {
      alert('Passwords do not match')
      return
    }
    if (password.length < 8) {
      alert('Password must be at least 8 characters')
      return
    }
    
    isCreatingAccount = true
    showPasswordModal = false
    try {
      const account = await invoke('create_chiral_account') as { address: string, private_key: string }
      
      // Save to encrypted keystore
      await invoke('save_account_to_keystore', {
        address: account.address,
        privateKey: account.private_key,
        password: password
      })
      
      // Update the Chiral account store
      etcAccount.set(account)
      // Also update the wallet store with the new Chiral address
      wallet.update(w => ({
        ...w,
        address: account.address
      }))
      // Show private key after creation
      privateKeyVisible = true
      
      // Reload saved accounts
      await loadSavedAccounts()
      
      // Clear password fields
      password = ''
      confirmPassword = ''
      
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
    passwordAction = 'import'
    showPasswordModal = true
  }

  async function confirmImportAccount() {
    if (password !== confirmPassword) {
      alert('Passwords do not match')
      return
    }
    if (password.length < 8) {
      alert('Password must be at least 8 characters')
      return
    }
    
    isImportingAccount = true
    showPasswordModal = false
    try {
      const account = await invoke('import_chiral_account', { privateKey: importPrivateKey }) as { address: string, private_key: string }
      
      // Save to encrypted keystore
      await invoke('save_account_to_keystore', {
        address: account.address,
        privateKey: account.private_key,
        password: password
      })
      
      // Update the Chiral account store
      etcAccount.set(account)
      // Also update the wallet store with the imported Chiral address
      wallet.update(w => ({
        ...w,
        address: account.address
      }))
      importPrivateKey = ''
      // Show private key after import
      privateKeyVisible = true
      
      // Reload saved accounts
      await loadSavedAccounts()
      
      // Clear password fields
      password = ''
      confirmPassword = ''
      
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

  async function unlockAccount() {
    if (!selectedSavedAccount) {
      alert('Please select an account to unlock')
      return
    }
    passwordAction = 'unlock'
    showPasswordModal = true
  }

  async function confirmUnlockAccount() {
    try {
      const account = await invoke('load_account_from_keystore', {
        address: selectedSavedAccount,
        password: unlockPassword
      }) as { address: string, private_key: string }
      
      // Update the Chiral account store
      etcAccount.set(account)
      // Also update the wallet store
      wallet.update(w => ({
        ...w,
        address: account.address
      }))
      
      showPasswordModal = false
      unlockPassword = ''
      privateKeyVisible = false
      
      // Fetch balance for unlocked account
      if (isGethRunning) {
        await fetchBalance()
      }
    } catch (error) {
      alert('Failed to unlock account: Incorrect password or corrupted keystore')
    }
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Account</h1>
    <p class="text-muted-foreground mt-2">Manage your wallet and account settings</p>
  </div>
  
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">Chiral Network Wallet</h2>
        <Wallet class="h-5 w-5 text-muted-foreground" />
      </div>
      
      <div class="space-y-4">
        {#if !$etcAccount}
          <div class="space-y-3">
            {#if savedAccounts.length > 0}
              <div class="space-y-2">
                <p class="text-sm text-muted-foreground">Saved Accounts:</p>
                <select 
                  bind:value={selectedSavedAccount}
                  class="w-full p-2 border rounded"
                >
                  <option value="">Select an account</option>
                  {#each savedAccounts as account}
                    <option value={account}>{account.slice(0, 10)}...{account.slice(-8)}</option>
                  {/each}
                </select>
                <Button 
                  class="w-full" 
                  variant="outline"
                  on:click={unlockAccount}
                  disabled={!selectedSavedAccount}
                >
                  <Key class="h-4 w-4 mr-2" />
                  Unlock Selected Account
                </Button>
              </div>
              
              <div class="relative">
                <div class="absolute inset-0 flex items-center">
                  <span class="w-full border-t" />
                </div>
                <div class="relative flex justify-center text-xs uppercase">
                  <span class="bg-background px-2 text-muted-foreground">Or</span>
                </div>
              </div>
            {:else}
              <p class="text-sm text-muted-foreground">No Chiral Network account yet. Create or import one:</p>
            {/if}
            
            <Button 
              class="w-full" 
              on:click={createChiralAccount}
              disabled={isCreatingAccount}
            >
              <Plus class="h-4 w-4 mr-2" />
              {isCreatingAccount ? 'Creating...' : 'Create New Chiral Account'}
            </Button>
            
            <div class="space-y-2">
              <Input
                type="password"
                bind:value={importPrivateKey}
                placeholder="Enter private key to import"
                class="w-full"
              />
              <Button 
                class="w-full" 
                variant="outline"
                on:click={importChiralAccount}
                disabled={!importPrivateKey || isImportingAccount}
              >
                <Import class="h-4 w-4 mr-2" />
                {isImportingAccount ? 'Importing...' : 'Import Chiral Account'}
              </Button>
            </div>
          </div>
        {:else}
          <div>
            <p class="text-sm text-muted-foreground">Chiral Address</p>
            <div class="flex items-center gap-2 mt-1">
              <p class="font-mono text-sm">{$etcAccount.address.slice(0, 10)}...{$etcAccount.address.slice(-8)}</p>
              <div class="flex flex-col items-center">
                <Button size="sm" variant="ghost" on:click={copyAddress}>
                  <Copy class="h-3 w-3" />
                </Button>
                {#if copyMessage}
                  <span class="text-xs text-muted-foreground mt-1">{copyMessage}</span>
                {/if}
              </div>
            </div>
            
            <div class="mt-3">
              <p class="text-sm text-muted-foreground">Balance</p>
              <div class="flex items-center gap-2 mt-1">
                {#if isLoadingBalance}
                  <p class="text-lg font-semibold">Loading...</p>
                {:else}
                  <p class="text-lg font-semibold">{accountBalance} CN</p>
                {/if}
                {#if isGethRunning}
                  <Button size="sm" variant="ghost" on:click={fetchBalance}>
                    <svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                    </svg>
                  </Button>
                {/if}
              </div>
            </div>
            
            <div class="mt-4">
              <p class="text-sm text-muted-foreground">Private Key</p>
              <div class="flex gap-2 mt-1">
                <Input
                  type={privateKeyVisible ? 'text' : 'password'}
                  value={$etcAccount.private_key}
                  readonly
                  class="flex-1 font-mono text-xs"
                />
                <Button
                  size="sm"
                  variant="ghost"
                  on:click={() => privateKeyVisible = !privateKeyVisible}
                >
                  {privateKeyVisible ? 'Hide' : 'Show'}
                </Button>
              </div>
              <p class="text-xs text-red-500 mt-1">⚠️ Never share your private key!</p>
            </div>
          </div>
        {/if}
        
      </div>
    </Card>
    
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">Send CN</h2>
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
              autocomplete="off"
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
              bind:value={sendAmount}
              placeholder="0.00"
              max={parseFloat(accountBalance)}
              class="mt-2"
              autocomplete="off"
              data-form-type="other"
              data-lpignore="true"
              aria-autocomplete="none"
            />
            {#if amountWarning}
              <p class="text-xs text-red-500 mt-1">{amountWarning}</p>
            {/if}
            <p class="text-xs text-muted-foreground mt-1">
              Available: {accountBalance} CN
            </p>
          </div>

          <Button
            type="button"
            class="w-full"
            on:click={sendTransaction}
            disabled={!recipientAddress || sendAmount <= 0 || sendAmount > parseFloat(accountBalance) || !$etcAccount || !isGethRunning}
          >
            <ArrowUpRight class="h-4 w-4 mr-2" />
            Send Transaction
          </Button>
        </div>
      </form>
    </Card>
  </div>
  
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Transaction History</h2>
      <History class="h-5 w-5 text-muted-foreground" />
    </div>
    <!-- Filter Controls -->
    <div class="flex flex-wrap gap-4 mb-4 items-end">
      <div>
        <label class="block text-xs font-medium mb-1">Type</label>
        <select bind:value={filterType} class="border rounded px-2 py-1 text-sm">
          <option value="all">All</option>
          <option value="sent">Sent</option>
          <option value="received">Received</option>
        </select>
      </div>
      <div>
        <label class="block text-xs font-medium mb-1">From</label>
        <input type="date" bind:value={filterDateFrom} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label class="block text-xs font-medium mb-1">To</label>
        <input type="date" bind:value={filterDateTo} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label class="block text-xs font-medium mb-1">Sort</label>
        <button type="button" class="border rounded px-3 py-1 text-sm bg-white hover:bg-gray-100 transition-colors w-full" on:click={() => { sortDescending = !sortDescending; }}>
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
  
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Security</h2>
      <Settings class="h-5 w-5 text-muted-foreground" />
    </div>

    <form autocomplete="off" data-form-type="other" data-lpignore="true">
      <div class="space-y-4">
        <div>
          <Label>Private Key (Chiral)</Label>
          <div class="flex gap-2 mt-2">
            <Input
              type={privateKeyVisible ? 'text' : 'password'}
              value={$etcAccount ? $etcAccount.private_key : 'No Chiral account created yet'}
              readonly
              class="flex-1 font-mono text-sm"
              autocomplete="off"
              data-form-type="other"
              data-lpignore="true"
              aria-autocomplete="none"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              on:click={() => privateKeyVisible = !privateKeyVisible}
            >
              {privateKeyVisible ? 'Hide' : 'Show'}
            </Button>
          </div>
          <p class="text-xs text-muted-foreground mt-1">Never share your private key with anyone</p>
        </div>

        <Button type="button" variant="outline" class="w-full">
          <Key class="h-4 w-4 mr-2" />
          Export Wallet
        </Button>
      </div>
    </form>
  </Card>
  
  <!-- Password Modal -->
  {#if showPasswordModal}
    <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
      <Card class="p-6 w-96 max-w-full">
        <h3 class="text-lg font-semibold mb-4">
          {#if passwordAction === 'create'}
            Set Password for New Account
          {:else if passwordAction === 'import'}
            Set Password for Imported Account
          {:else}
            Enter Password to Unlock Account
          {/if}
        </h3>
        
        <div class="space-y-4">
          {#if passwordAction === 'unlock'}
            <div>
              <Label for="unlock-password">Password</Label>
              <Input
                id="unlock-password"
                type="password"
                bind:value={unlockPassword}
                placeholder="Enter your password"
                class="mt-2"
              />
            </div>
          {:else}
            <div>
              <Label for="password">Password</Label>
              <Input
                id="password"
                type="password"
                bind:value={password}
                placeholder="Enter a strong password"
                class="mt-2"
              />
              <p class="text-xs text-muted-foreground mt-1">Minimum 8 characters</p>
            </div>
            
            <div>
              <Label for="confirm-password">Confirm Password</Label>
              <Input
                id="confirm-password"
                type="password"
                bind:value={confirmPassword}
                placeholder="Confirm your password"
                class="mt-2"
              />
            </div>
          {/if}
          
          <div class="flex gap-2">
            <Button
              variant="outline"
              class="flex-1"
              on:click={() => {
                showPasswordModal = false
                password = ''
                confirmPassword = ''
                unlockPassword = ''
              }}
            >
              Cancel
            </Button>
            <Button
              class="flex-1"
              on:click={() => {
                if (passwordAction === 'create') {
                  confirmCreateAccount()
                } else if (passwordAction === 'import') {
                  confirmImportAccount()
                } else {
                  confirmUnlockAccount()
                }
              }}
            >
              {passwordAction === 'unlock' ? 'Unlock' : 'Confirm'}
            </Button>
          </div>
        </div>
      </Card>
    </div>
  {/if}
</div>