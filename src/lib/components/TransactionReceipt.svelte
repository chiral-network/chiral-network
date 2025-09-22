<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { 
    X, 
    Copy, 
    Calendar, 
    Hash, 
    Clock, 
    CheckCircle, 
    XCircle,
    ArrowUpRight,
    ArrowDownLeft,
    FileText,
    Download,
    Zap
  } from 'lucide-svelte'
  import { t } from 'svelte-i18n'
  import { get } from 'svelte/store'
  import { showToast } from '$lib/toast'

  const tr = (k: string, params?: Record<string, any>) => get(t)(k, params)

  export let transaction: any = null
  export let isOpen = false
  export let onClose: () => void = () => {}

  let showRawData = false

  // Mock transaction data - in real app this would come from backend
  $: txData = transaction ? {
    ...transaction,
    hash: transaction.hash || `0x${Math.random().toString(16).substr(2, 64)}`,
    blockNumber: transaction.blockNumber || Math.floor(Math.random() * 1000000),
    gasUsed: transaction.gasUsed || Math.floor(Math.random() * 21000),
    gasPrice: transaction.gasPrice || Math.floor(Math.random() * 20) + 1,
    nonce: transaction.nonce || Math.floor(Math.random() * 1000),
    fee: transaction.fee || (transaction.amount * 0.001),
    timestamp: transaction.timestamp || transaction.date.getTime(),
    confirmations: transaction.confirmations || Math.floor(Math.random() * 12) + 1
  } : null

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

  function copyToClipboard(text: string, label: string) {
    navigator.clipboard.writeText(text).then(() => {
      showToast(`${label} copied to clipboard`, 'success')
    }).catch(() => {
      showToast(`Failed to copy ${label}`, 'error')
    })
  }

  function formatAddress(address: string) {
    if (!address) return ''
    return `${address.slice(0, 6)}...${address.slice(-4)}`
  }

  function formatAmount(amount: number) {
    return amount.toFixed(6)
  }

  function formatDate(date: Date) {
    return new Intl.DateTimeFormat('en-US', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit'
    }).format(date)
  }

  function formatGasPrice(gasPrice: number) {
    return `${gasPrice} Gwei`
  }

  function exportTransaction() {
    if (!txData) return
    
    const exportData = {
      hash: txData.hash,
      from: txData.from,
      to: txData.to,
      amount: txData.amount,
      type: txData.type,
      status: txData.status,
      blockNumber: txData.blockNumber,
      gasUsed: txData.gasUsed,
      gasPrice: txData.gasPrice,
      fee: txData.fee,
      timestamp: txData.timestamp,
      description: txData.description
    }

    const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `transaction-${txData.hash.slice(0, 8)}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
    
    showToast('Transaction exported', 'success')
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose()
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose()
    }
  }
</script>

{#if isOpen && txData}
  <!-- Backdrop -->
  <div 
    class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center p-4"
    on:click={handleBackdropClick}
    on:keydown={handleKeydown}
    role="dialog"
    aria-modal="true"
    aria-labelledby="receipt-title"
    tabindex="0"
  >
  <!-- Modal -->
  <div class="w-full max-w-3xl max-h-[90vh] overflow-hidden bg-white dark:bg-gray-900 rounded-xl shadow-2xl border border-gray-200 dark:border-gray-700 flex flex-col relative">
    <!-- Header -->
    <div class="absolute top-0 left-0 right-0 z-20 bg-white dark:bg-gray-900 border-b border-gray-200 dark:border-gray-700 rounded-t-xl shadow-lg">
      <div class="flex items-center justify-between p-6">
        <div class="flex items-center space-x-4">
          <div class="p-2 rounded-lg {txData.type === 'sent' ? 'bg-red-100 dark:bg-red-900/20' : 'bg-green-100 dark:bg-green-900/20'}">
            <svelte:component 
              this={getTypeIcon(txData.type)} 
              class="w-6 h-6 {getTypeColor(txData.type)}"
            />
          </div>
          <div>
            <h2 id="receipt-title" class="text-2xl font-bold text-gray-900 dark:text-gray-100">
              {tr('transactions.receipt.title')}
            </h2>
            <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Transaction Details
            </p>
          </div>
        </div>
        <div class="flex items-center space-x-3">
          <Button
            variant="outline"
            size="sm"
            on:click={exportTransaction}
            class="flex items-center space-x-2 px-4 py-2"
          >
            <Download class="w-4 h-4" />
            <span>Export</span>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            on:click={onClose}
            class="p-2 hover:bg-red-100 dark:hover:bg-red-900/20 rounded-lg border border-gray-200 dark:border-gray-700 hover:border-red-300 dark:hover:border-red-700 transition-all duration-200"
            title="Close modal"
          >
            <X class="w-5 h-5 text-gray-600 dark:text-gray-400 hover:text-red-600 dark:hover:text-red-400" />
          </Button>
        </div>
      </div>
    </div>

      <!-- Content -->
      <div class="flex-1 overflow-y-auto p-6 space-y-6 relative z-0 mt-20">
        <!-- Status and Amount -->
        <div class="bg-gradient-to-r from-gray-50 to-gray-100 dark:from-gray-800 dark:to-gray-700 rounded-xl p-6">
          <div class="flex items-center justify-between">
            <div class="flex items-center space-x-4">
              <div class="flex items-center space-x-3">
                <Badge class="{getStatusColor(txData.status)} px-3 py-1 text-sm font-medium">
                  <svelte:component 
                    this={getStatusIcon(txData.status)} 
                    class="w-4 h-4 mr-2"
                  />
                  {txData.status}
                </Badge>
                <div class="flex items-center space-x-2 text-sm text-gray-600 dark:text-gray-300">
                  <Clock class="w-4 h-4" />
                  <span>{txData.confirmations} confirmations</span>
                </div>
              </div>
            </div>
            <div class="text-right">
              <div class="text-3xl font-bold {getTypeColor(txData.type)} mb-1">
                {txData.type === 'sent' ? '-' : '+'}{formatAmount(txData.amount)} CHR
              </div>
              <div class="text-sm text-gray-600 dark:text-gray-300 font-medium">
                {txData.description || tr('transactions.manual')}
              </div>
            </div>
          </div>
        </div>

        <!-- Transaction Details -->
        <div class="space-y-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 flex items-center">
            <Hash class="w-5 h-5 mr-2 text-blue-600" />
            Transaction Details
          </h3>
          
          <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <!-- From Address -->
            <div class="space-y-2">
              <label for="from-address" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.from')}
              </label>
              <div class="group relative">
                <div class="flex items-center space-x-3 p-4 bg-gray-50 dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors">
                  <div class="w-10 h-10 bg-blue-100 dark:bg-blue-900/20 rounded-lg flex items-center justify-center">
                    <ArrowDownLeft class="w-5 h-5 text-blue-600" />
                  </div>
                  <code id="from-address" class="flex-1 text-sm font-mono text-gray-900 dark:text-gray-100 truncate">
                    {formatAddress(txData.from || '')}
                  </code>
                  <Button
                    variant="ghost"
                    size="sm"
                    on:click={() => copyToClipboard(txData.from || '', 'From address')}
                    class="opacity-0 group-hover:opacity-100 transition-opacity p-2 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg"
                  >
                    <Copy class="w-4 h-4" />
                  </Button>
                </div>
              </div>
            </div>

            <!-- To Address -->
            <div class="space-y-2">
              <label for="to-address" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.to')}
              </label>
              <div class="group relative">
                <div class="flex items-center space-x-3 p-4 bg-gray-50 dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors">
                  <div class="w-10 h-10 bg-green-100 dark:bg-green-900/20 rounded-lg flex items-center justify-center">
                    <ArrowUpRight class="w-5 h-5 text-green-600" />
                  </div>
                  <code id="to-address" class="flex-1 text-sm font-mono text-gray-900 dark:text-gray-100 truncate">
                    {formatAddress(txData.to || '')}
                  </code>
                  <Button
                    variant="ghost"
                    size="sm"
                    on:click={() => copyToClipboard(txData.to || '', 'To address')}
                    class="opacity-0 group-hover:opacity-100 transition-opacity p-2 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg"
                  >
                    <Copy class="w-4 h-4" />
                  </Button>
                </div>
              </div>
            </div>

            <!-- Transaction Hash -->
            <div class="space-y-2">
              <label for="tx-hash" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.hash')}
              </label>
              <div class="group relative">
                <div class="flex items-center space-x-3 p-4 bg-gray-50 dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors">
                  <div class="w-10 h-10 bg-purple-100 dark:bg-purple-900/20 rounded-lg flex items-center justify-center">
                    <Hash class="w-5 h-5 text-purple-600" />
                  </div>
                  <code id="tx-hash" class="flex-1 text-sm font-mono text-gray-900 dark:text-gray-100 truncate">
                    {formatAddress(txData.hash)}
                  </code>
                  <Button
                    variant="ghost"
                    size="sm"
                    on:click={() => copyToClipboard(txData.hash, 'Transaction hash')}
                    class="opacity-0 group-hover:opacity-100 transition-opacity p-2 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg"
                  >
                    <Copy class="w-4 h-4" />
                  </Button>
                </div>
              </div>
            </div>

            <!-- Block Number -->
            <div class="space-y-2">
              <label for="block-number" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.blockNumber')}
              </label>
              <div class="group relative">
                <div class="flex items-center space-x-3 p-4 bg-gray-50 dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors">
                  <div class="w-10 h-10 bg-orange-100 dark:bg-orange-900/20 rounded-lg flex items-center justify-center">
                    <span class="text-orange-600 font-bold text-sm">#</span>
                  </div>
                  <code id="block-number" class="flex-1 text-sm font-mono text-gray-900 dark:text-gray-100">
                    {txData.blockNumber.toLocaleString()}
                  </code>
                  <Button
                    variant="ghost"
                    size="sm"
                    on:click={() => copyToClipboard(txData.blockNumber.toString(), 'Block number')}
                    class="opacity-0 group-hover:opacity-100 transition-opacity p-2 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg"
                  >
                    <Copy class="w-4 h-4" />
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Gas Information -->
        <div class="space-y-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 flex items-center">
            <Zap class="w-5 h-5 mr-2 text-yellow-600" />
            {tr('transactions.receipt.gasInfo')}
          </h3>
          <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
            <div class="space-y-2">
              <label for="gas-used" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.gasUsed')}
              </label>
              <div class="p-4 bg-gradient-to-r from-blue-50 to-blue-100 dark:from-blue-900/20 dark:to-blue-800/20 rounded-xl border border-blue-200 dark:border-blue-700">
                <div class="flex items-center space-x-3">
                  <div class="w-8 h-8 bg-blue-500 rounded-lg flex items-center justify-center">
                    <span class="text-white font-bold text-sm">G</span>
                  </div>
                  <code id="gas-used" class="text-lg font-mono font-bold text-blue-900 dark:text-blue-100">
                    {txData.gasUsed.toLocaleString()}
                  </code>
                </div>
              </div>
            </div>
            <div class="space-y-2">
              <label for="gas-price" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.gasPrice')}
              </label>
              <div class="p-4 bg-gradient-to-r from-green-50 to-green-100 dark:from-green-900/20 dark:to-green-800/20 rounded-xl border border-green-200 dark:border-green-700">
                <div class="flex items-center space-x-3">
                  <div class="w-8 h-8 bg-green-500 rounded-lg flex items-center justify-center">
                    <span class="text-white font-bold text-sm">P</span>
                  </div>
                  <code id="gas-price" class="text-lg font-mono font-bold text-green-900 dark:text-green-100">
                    {formatGasPrice(txData.gasPrice)}
                  </code>
                </div>
              </div>
            </div>
            <div class="space-y-2">
              <label for="fee" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.fee')}
              </label>
              <div class="p-4 bg-gradient-to-r from-purple-50 to-purple-100 dark:from-purple-900/20 dark:to-purple-800/20 rounded-xl border border-purple-200 dark:border-purple-700">
                <div class="flex items-center space-x-3">
                  <div class="w-8 h-8 bg-purple-500 rounded-lg flex items-center justify-center">
                    <span class="text-white font-bold text-sm">F</span>
                  </div>
                  <code id="fee" class="text-lg font-mono font-bold text-purple-900 dark:text-purple-100">
                    {formatAmount(txData.fee)} CHR
                  </code>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Timestamp and Nonce -->
        <div class="space-y-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 flex items-center">
            <Calendar class="w-5 h-5 mr-2 text-indigo-600" />
            Additional Information
          </h3>
          
          <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <div class="space-y-2">
              <label for="timestamp" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.timestamp')}
              </label>
              <div id="timestamp" class="p-4 bg-gradient-to-r from-indigo-50 to-indigo-100 dark:from-indigo-900/20 dark:to-indigo-800/20 rounded-xl border border-indigo-200 dark:border-indigo-700">
                <div class="flex items-center space-x-3">
                  <div class="w-8 h-8 bg-indigo-500 rounded-lg flex items-center justify-center">
                    <Calendar class="w-4 h-4 text-white" />
                  </div>
                  <div>
                    <div class="text-sm font-medium text-indigo-900 dark:text-indigo-100">
                      {formatDate(new Date(txData.timestamp))}
                    </div>
                    <div class="text-xs text-indigo-600 dark:text-indigo-300">
                      {new Date(txData.timestamp).toLocaleTimeString()}
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <div class="space-y-2">
              <label for="nonce" class="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {tr('transactions.receipt.nonce')}
              </label>
              <div class="p-4 bg-gradient-to-r from-gray-50 to-gray-100 dark:from-gray-800 dark:to-gray-700 rounded-xl border border-gray-200 dark:border-gray-600">
                <div class="flex items-center space-x-3">
                  <div class="w-8 h-8 bg-gray-500 rounded-lg flex items-center justify-center">
                    <span class="text-white font-bold text-sm">N</span>
                  </div>
                  <code id="nonce" class="text-lg font-mono font-bold text-gray-900 dark:text-gray-100">
                    {txData.nonce}
                  </code>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Raw Data Toggle -->
        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 flex items-center">
              <FileText class="w-5 h-5 mr-2 text-gray-600" />
              Raw Transaction Data
            </h3>
            <Button
              variant="outline"
              on:click={() => showRawData = !showRawData}
              class="flex items-center space-x-2 px-4 py-2"
            >
              <FileText class="w-4 h-4" />
              <span>{showRawData ? 'Hide' : 'Show'} Raw Data</span>
            </Button>
          </div>
          
          {#if showRawData}
            <div class="p-4 bg-gray-900 dark:bg-gray-800 rounded-xl border border-gray-700">
              <pre class="text-xs font-mono text-green-400 overflow-x-auto whitespace-pre-wrap">{JSON.stringify(txData, null, 2)}</pre>
            </div>
          {/if}
        </div>
      </div>

      <!-- Footer -->
      <div class="sticky bottom-0 z-20 bg-white dark:bg-gray-900 border-t border-gray-200 dark:border-gray-700 rounded-b-xl shadow-lg">
        <div class="flex justify-between items-center p-6">
          <div class="text-sm text-gray-500 dark:text-gray-400">
            Transaction ID: {txData.hash.slice(0, 8)}...{txData.hash.slice(-8)}
          </div>
          <div class="flex space-x-3">
            <Button
              variant="outline"
              on:click={onClose}
              class="px-6 py-2"
            >
              {tr('common.close')}
            </Button>
            <Button
              on:click={() => {
                copyToClipboard(txData.hash, 'Transaction hash')
                onClose()
              }}
              class="flex items-center space-x-2 px-6 py-2"
            >
              <Copy class="w-4 h-4" />
              <span>Copy Hash</span>
            </Button>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}