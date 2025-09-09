<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { Wallet, Copy, ArrowUpRight, ArrowDownLeft, Settings, Key, History, Coins } from 'lucide-svelte'
  import { wallet } from '$lib/stores'
  import { writable, derived } from 'svelte/store'
  
  let recipientAddress = ''
  let sendAmount = 0
  let privateKeyVisible = false
  let showPending = false
  
  const transactions = writable([
  { id: 1, type: 'received', amount: 50.5, from: '0x8765...4321', date: new Date('2024-03-15'), description: 'File purchase', status: 'completed' },
  { id: 2, type: 'sent', amount: 10.25, to: '0x1234...5678', date: new Date('2024-03-14'), description: 'Proxy service', status: 'completed' },
  { id: 3, type: 'received', amount: 100, from: '0xabcd...ef12', date: new Date('2024-03-13'), description: 'Upload reward', status: 'completed' },
  { id: 4, type: 'sent', amount: 5.5, to: '0x9876...5432', date: new Date('2024-03-12'), description: 'File download', status: 'completed' },
  ]);

  // Warning message for amount input
  let amountWarning = ''

  // Copy feedback message
  let copyMessage = ''

  $: {
    const prevAmount = sendAmount
    sendAmount = Math.max(0.01, Math.min(sendAmount, $wallet.balance))
    amountWarning = (prevAmount !== sendAmount)
      ? `Amount cannot be ${prevAmount}. Allowed range: 0.01-${$wallet.balance.toFixed(2)} CN.`
      : ''
  }
  
  function copyAddress() {
    navigator.clipboard.writeText($wallet.address);
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
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Account</h1>
    <p class="text-muted-foreground mt-2">Manage your wallet and account settings</p>
  </div>
  
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">Wallet</h2>
        <Wallet class="h-5 w-5 text-muted-foreground" />
      </div>
      
      <div class="space-y-4">
        <div>
          <p class="text-sm text-muted-foreground">Address</p>
          <div class="flex items-center gap-2 mt-1">
            <p class="font-mono text-sm">{$wallet.address.slice(0, 10)}...{$wallet.address.slice(-8)}</p>
            <div class="flex flex-col items-center">
              <Button size="sm" variant="ghost" on:click={copyAddress}>
                <Copy class="h-3 w-3" />
              </Button>
              {#if copyMessage}
                <span class="text-xs text-muted-foreground mt-1">{copyMessage}</span>
              {/if}
            </div>
          </div>
        </div>
        
        <div>
          <p class="text-sm text-muted-foreground">Balance</p>
          <p class="text-2xl font-bold">{$wallet.balance.toFixed(2)} CN</p>
        </div>
        
        <div class="grid grid-cols-2 gap-4">
          <div>
            <p class="text-xs text-muted-foreground">Total Earned</p>
            <p class="text-sm font-medium text-green-600">+{$wallet.totalEarned.toFixed(2)} CN</p>
          </div>
          <div>
            <p class="text-xs text-muted-foreground">Total Spent</p>
            <p class="text-sm font-medium text-red-600">-{$wallet.totalSpent.toFixed(2)} CN</p>
          </div>
        </div>
        
        <Button class="w-full justify-center bg-gray-100 hover:bg-gray-200 text-gray-800 rounded transition-colors py-2 font-normal" on:click={() => showPending = !showPending} aria-label="View pending transactions">
          <span class="flex items-center gap-2">
            <svg class="h-4 w-4 text-gray-500" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" /></svg>
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
    </Card>
    
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
              max={$wallet.balance}
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
              Available: {$wallet.balance.toFixed(2)} CN
            </p>
          </div>

          <Button
            type="button"
            class="w-full"
            on:click={sendTransaction}
            disabled={!recipientAddress || sendAmount <= 0 || sendAmount > $wallet.balance}
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
    
    <div class="space-y-2">
      {#each $transactions as tx}
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
          <Label>Private Key</Label>
          <div class="flex gap-2 mt-2">
            <Input
              type={privateKeyVisible ? 'text' : 'password'}
              value="your-private-key-here-do-not-share"
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
</div>