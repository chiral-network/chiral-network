<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import DropDown from '$lib/components/ui/dropDown.svelte'
  import { 
    ArrowUpRight, 
    ArrowDownLeft, 
    Clock, 
    CheckCircle, 
    XCircle, 
    Search, 
    ExternalLink
  } from 'lucide-svelte'
  import { t } from 'svelte-i18n'
  import { get } from 'svelte/store'

  const tr = (k: string, params?: Record<string, any>) => get(t)(k, params)

  export let transactions: any[] = []
  export let onTransactionClick: (tx: any) => void = () => {}
  export let showFilters = true
  export let compact = false

  // Filter states
  let searchQuery = ''
  let statusFilter = 'all'
  let typeFilter = 'all'
  let sortBy = 'date'
  let sortOrder = 'desc'

  // Available filter options
  const statusOptions = [
    { value: 'all', label: 'All Status' },
    { value: 'pending', label: 'Pending' },
    { value: 'completed', label: 'Completed' },
    { value: 'failed', label: 'Failed' }
  ]

  const typeOptions = [
    { value: 'all', label: 'All Types' },
    { value: 'sent', label: 'Sent' },
    { value: 'received', label: 'Received' }
  ]

  const sortOptions = [
    { value: 'date', label: 'Date' },
    { value: 'amount', label: 'Amount' },
    { value: 'status', label: 'Status' }
  ]

  // Computed filtered and sorted transactions
  $: filteredTransactions = transactions
    .filter(tx => {
      // Search filter
      if (searchQuery) {
        const query = searchQuery.toLowerCase()
        const matchesSearch = 
          tx.description?.toLowerCase().includes(query) ||
          tx.to?.toLowerCase().includes(query) ||
          tx.from?.toLowerCase().includes(query) ||
          tx.id.toString().includes(query)
        if (!matchesSearch) return false
      }

      // Status filter
      if (statusFilter !== 'all' && tx.status !== statusFilter) return false

      // Type filter
      if (typeFilter !== 'all' && tx.type !== typeFilter) return false

      return true
    })
    .sort((a, b) => {
      let comparison = 0
      
      switch (sortBy) {
        case 'date':
          comparison = new Date(a.date).getTime() - new Date(b.date).getTime()
          break
        case 'amount':
          comparison = a.amount - b.amount
          break
        case 'status':
          comparison = a.status.localeCompare(b.status)
          break
      }

      return sortOrder === 'desc' ? -comparison : comparison
    })

  function getStatusIcon(status: string) {
    switch (status) {
      case 'pending':
        return Clock
      case 'completed':
        return CheckCircle
      case 'failed':
        return XCircle
      default:
        return Clock
    }
  }

  function getStatusColor(status: string) {
    switch (status) {
      case 'pending':
        return 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/20 dark:text-yellow-400'
      case 'completed':
        return 'bg-green-100 text-green-800 dark:bg-green-900/20 dark:text-green-400'
      case 'failed':
        return 'bg-red-100 text-red-800 dark:bg-red-900/20 dark:text-red-400'
      default:
        return 'bg-gray-100 text-gray-800 dark:bg-gray-900/20 dark:text-gray-400'
    }
  }

  function getTypeIcon(type: string) {
    return type === 'sent' ? ArrowUpRight : ArrowDownLeft
  }

  function getTypeColor(type: string) {
    return type === 'sent' 
      ? 'text-red-600 dark:text-red-400' 
      : 'text-green-600 dark:text-green-400'
  }

  function formatAmount(amount: number) {
    return amount.toFixed(4)
  }

  function formatDate(date: Date) {
    const now = new Date()
    const diffMs = now.getTime() - date.getTime()
    const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))
    
    if (diffDays === 0) {
      return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    } else if (diffDays === 1) {
      return 'Yesterday'
    } else if (diffDays < 7) {
      return `${diffDays} days ago`
    } else {
      return date.toLocaleDateString()
    }
  }

  // Removed unused copyToClipboard function

  function handleTransactionClick(tx: any) {
    onTransactionClick(tx)
  }

  function clearFilters() {
    searchQuery = ''
    statusFilter = 'all'
    typeFilter = 'all'
    sortBy = 'date'
    sortOrder = 'desc'
  }
</script>

<div class="space-y-4">
  {#if showFilters}
    <!-- Filters and Search -->
    <Card class="p-4">
      <div class="flex flex-col sm:flex-row gap-4">
        <!-- Search -->
        <div class="flex-1">
          <div class="relative">
            <Search class="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
            <Input
              bind:value={searchQuery}
              placeholder={tr('transactions.searchPlaceholder')}
              class="pl-10"
            />
          </div>
        </div>

        <!-- Filters -->
        <div class="flex gap-2">
          <DropDown
            options={statusOptions}
            bind:value={statusFilter}
            placeholder="Status"
            className="min-w-[120px]"
          />
          <DropDown
            options={typeOptions}
            bind:value={typeFilter}
            placeholder="Type"
            className="min-w-[120px]"
          />
          <DropDown
            options={sortOptions}
            bind:value={sortBy}
            placeholder="Sort by"
            className="min-w-[120px]"
          />
          <Button
            variant="outline"
            size="sm"
            on:click={() => sortOrder = sortOrder === 'desc' ? 'asc' : 'desc'}
            class="px-3"
          >
            {sortOrder === 'desc' ? '↓' : '↑'}
          </Button>
          <Button
            variant="outline"
            size="sm"
            on:click={clearFilters}
            class="px-3"
          >
            Clear
          </Button>
        </div>
      </div>
    </Card>
  {/if}

  <!-- Transaction List -->
  <div class="space-y-2">
    {#if filteredTransactions.length === 0}
      <Card class="p-8 text-center">
        <div class="text-gray-500 dark:text-gray-400">
          {#if searchQuery || statusFilter !== 'all' || typeFilter !== 'all'}
            {tr('transactions.noResults')}
          {:else}
            {tr('transactions.empty')}
          {/if}
        </div>
      </Card>
    {:else}
      {#each filteredTransactions as tx (tx.id)}
        <Card 
          class="p-4 hover:bg-gray-50 dark:hover:bg-gray-800/50 cursor-pointer transition-colors"
          on:click={() => handleTransactionClick(tx)}
          on:keydown={(e) => {
            if ((e as unknown as KeyboardEvent).key === 'Enter') {
              handleTransactionClick(tx)
            }
          }}
          role="button"
          tabindex="0"
        >
          <div class="flex items-center justify-between">
            <!-- Left side: Type, Description, Address -->
            <div class="flex items-center space-x-3 flex-1 min-w-0">
              <!-- Type Icon -->
              <div class="flex-shrink-0">
                <svelte:component 
                  this={getTypeIcon(tx.type)} 
                  class="w-5 h-5 {getTypeColor(tx.type)}"
                />
              </div>

              <!-- Description and Address -->
              <div class="flex-1 min-w-0">
                <div class="font-medium text-gray-900 dark:text-gray-100 truncate">
                  {tx.description || tr('transactions.manual')}
                </div>
                <div class="text-sm text-gray-500 dark:text-gray-400 truncate">
                  {tx.type === 'sent' ? tx.to : tx.from}
                </div>
              </div>
            </div>

            <!-- Right side: Amount, Status, Date -->
            <div class="flex items-center space-x-3 flex-shrink-0">
              <!-- Amount -->
              <div class="text-right">
                <div class="font-medium {getTypeColor(tx.type)}">
                  {tx.type === 'sent' ? '-' : '+'}{formatAmount(tx.amount)} CHR
                </div>
                <div class="text-xs text-gray-500 dark:text-gray-400">
                  {formatDate(tx.date)}
                </div>
              </div>

              <!-- Status Badge -->
              <Badge class={getStatusColor(tx.status)}>
                <svelte:component 
                  this={getStatusIcon(tx.status)} 
                  class="w-3 h-3 mr-1"
                />
                {tx.status}
              </Badge>

              <!-- External Link Icon -->
              <ExternalLink class="w-4 h-4 text-gray-400" />
            </div>
          </div>
        </Card>
      {/each}
    {/if}
  </div>

  {#if !compact && filteredTransactions.length > 0}
    <!-- Summary -->
    <Card class="p-4 bg-gray-50 dark:bg-gray-800/50">
      <div class="flex justify-between text-sm text-gray-600 dark:text-gray-400">
        <span>
          {filteredTransactions.length} {filteredTransactions.length === 1 ? 'transaction' : 'transactions'}
        </span>
        <span>
          {#if filteredTransactions.length > 0}
            Total: {filteredTransactions.reduce((sum, tx) => sum + (tx.type === 'sent' ? -tx.amount : tx.amount), 0).toFixed(4)} CHR
          {/if}
        </span>
      </div>
    </Card>
  {/if}
</div>
