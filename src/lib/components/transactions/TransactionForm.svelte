<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import Button from '$lib/components/ui/button.svelte';
  import Card from '$lib/components/ui/card.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import GasEstimator from './GasEstimator.svelte';
  import { Send, AlertCircle, CheckCircle, Loader, ArrowLeft } from 'lucide-svelte';
  import { wallet, etcAccount, addTransactionWithPolling } from '$lib/stores';
  import {
    broadcastTransaction,
    getNetworkStatus,
    getAddressNonce,
    TransactionServiceError,
    type NetworkStatus
  } from '$lib/services/transactionService';
  import {
    signTransaction,
    isValidAddress,
    formatEther,
    parseEther
  } from '$lib/services/walletService';
  import { showToast } from '$lib/toast';

  // Form state
  let recipientAddress = '';
  let sendAmount = '';
  let description = '';

  // Gas state from estimator
  let selectedGasPriceWei = '0';
  let estimatedGasLimit = 21000;
  let selectedSpeed: 'slow' | 'standard' | 'fast' = 'standard';
  let totalGasCostWei = '0';
  let totalGasCostEth = '0';

  // UI state
  type FormStep = 'input' | 'review' | 'signing' | 'broadcasting' | 'success';
  let formStep: FormStep = 'input';
  let isProcessing = false;
  let networkStatus: NetworkStatus | null = null;
  let validationErrors: Record<string, string> = {};
  let transactionHash: string | null = null;
  let currentNonce: number | null = null;

  // Reactive validations
  $: isAddressValid = recipientAddress && isValidAddress(recipientAddress);
  $: amountNum = parseFloat(sendAmount) || 0;
  $: isAmountValid = amountNum > 0 && amountNum <= $wallet.balance;
  $: hasGasSelection = Number(selectedGasPriceWei) > 0;
  $: totalCostEth = amountNum + parseFloat(totalGasCostEth);
  $: canAffordTotal = totalCostEth <= $wallet.balance;

  /**
   * Validate all form inputs
   */
  function validateForm(): boolean {
    validationErrors = {};

    if (!recipientAddress) {
      validationErrors.recipient = 'Recipient address is required';
    } else if (!isAddressValid) {
      validationErrors.recipient = 'Invalid Ethereum address format';
    }

    if (!sendAmount || amountNum <= 0) {
      validationErrors.amount = 'Amount must be greater than 0';
    } else if (amountNum > $wallet.balance) {
      validationErrors.amount = `Insufficient balance (available: ${$wallet.balance.toFixed(6)} CHR)`;
    } else if (!canAffordTotal) {
      validationErrors.amount = `Insufficient balance for amount + gas (need: ${totalCostEth.toFixed(6)} CHR)`;
    }

    if (!networkStatus?.chain_id || networkStatus.chain_id !== 98765) {
      validationErrors.network = 'Not connected to Chiral Network (Chain ID 98765)';
    }

    if (!hasGasSelection) {
      validationErrors.gas = 'Please select a gas price';
    }

    return Object.keys(validationErrors).length === 0;
  }

  /**
   * Handle gas selection from estimator
   */
  function handleGasSelected(event: CustomEvent<{
    gasPriceWei: string;
    estimatedGas: number;
    speed: 'slow' | 'standard' | 'fast';
    totalCostWei: string;
    totalCostEth: string;
  }>) {
    selectedGasPriceWei = event.detail.gasPriceWei;
    estimatedGasLimit = event.detail.estimatedGas;
    selectedSpeed = event.detail.speed;
    totalGasCostWei = event.detail.totalCostWei;
    totalGasCostEth = event.detail.totalCostEth;
  }

  /**
   * Move to review step
   */
  async function proceedToReview() {
    if (!validateForm()) {
      showToast('Please fix all errors before continuing', 'error');
      return;
    }

    // Fetch current nonce for the transaction
    try {
      const address = $etcAccount?.address;
      if (!address) {
        showToast('No wallet connected', 'error');
        return;
      }
      currentNonce = await getAddressNonce(address);
    } catch (error) {
      console.error('Failed to fetch nonce:', error);
      showToast('Failed to prepare transaction', 'error');
      return;
    }

    formStep = 'review';
  }

  /**
   * Go back to input
   */
  function backToInput() {
    formStep = 'input';
    validationErrors = {};
  }

  /**
   * Execute the transaction
   */
  async function executeTransaction() {
    const account = get(etcAccount);
    if (!account?.address) {
      showToast('No wallet connected', 'error');
      return;
    }

    if (!validateForm()) {
      showToast('Please fix validation errors', 'error');
      formStep = 'input';
      return;
    }

    isProcessing = true;

    try {
      // Step 1: Sign the transaction
      formStep = 'signing';
      showToast('Signing transaction...', 'info');

      const signedTx = await signTransaction({
        from: account.address,
        to: recipientAddress,
        value: sendAmount,
        gasLimit: estimatedGasLimit,
        gasPrice: Number(selectedGasPriceWei),
        nonce: currentNonce || undefined
      });

      // Step 2: Broadcast to network
      formStep = 'broadcasting';
      showToast('Broadcasting to network...', 'info');

      const response = await broadcastTransaction(signedTx);
      transactionHash = response.transaction_hash;

      // Step 3: Add to store and start polling
      const newTransaction = {
        id: Date.now(),
        type: 'sent' as const,
        amount: amountNum,
        to: recipientAddress,
        from: account.address,
        date: new Date(),
        description: description || `Send ${sendAmount} CHR`,
        status: 'submitted' as const,
        transaction_hash: response.transaction_hash,
        gas_price: Number(selectedGasPriceWei),
        gas_used: estimatedGasLimit,
        confirmations: 0,
        nonce: currentNonce || undefined,
        fee: Number(totalGasCostWei)
      };

      // Start polling in background
      addTransactionWithPolling(newTransaction).catch(err => {
        console.error('Polling failed:', err);
      });

      // Success!
      formStep = 'success';
      showToast(`Transaction submitted! Hash: ${response.transaction_hash.slice(0, 10)}...`, 'success');

    } catch (error) {
      console.error('Transaction failed:', error);

      if (error instanceof TransactionServiceError) {
        // Show user-friendly error message
        showToast(error.getUserMessage(), 'error');

        // Handle specific error cases
        switch (error.code) {
          case 'NONCE_TOO_LOW':
          case 'NONCE_TOO_HIGH':
            // Refresh nonce and go back to review
            try {
              currentNonce = await getAddressNonce(account.address);
              showToast('Nonce updated. Please try again.', 'info');
            } catch {}
            formStep = 'review';
            break;

          case 'INSUFFICIENT_FUNDS':
            validationErrors.amount = error.getUserMessage();
            formStep = 'input';
            break;

          case 'GAS_PRICE_TOO_LOW':
            showToast('Please select a higher gas price', 'warning');
            formStep = 'review';
            break;

          default:
            formStep = 'input';
        }
      } else {
        showToast(
          'Transaction failed: ' + (error instanceof Error ? error.message : 'Unknown error'),
          'error'
        );
        formStep = 'input';
      }
    } finally {
      isProcessing = false;
    }
  }

  /**
   * Reset form for new transaction
   */
  function resetForm() {
    recipientAddress = '';
    sendAmount = '';
    description = '';
    selectedGasPriceWei = '0';
    estimatedGasLimit = 21000;
    transactionHash = null;
    currentNonce = null;
    validationErrors = {};
    formStep = 'input';
  }

  /**
   * Initialize network status
   */
  onMount(async () => {
    try {
      networkStatus = await getNetworkStatus();

      if (networkStatus.chain_id !== 98765) {
        showToast('Warning: Not connected to Chiral Network', 'warning');
      }
    } catch (error) {
      console.error('Failed to get network status:', error);
      showToast('Failed to connect to network', 'error');
    }
  });
</script>

<div class="space-y-4">
  {#if formStep === 'input'}
    <!-- Input Form -->
    <Card class="p-6">
      <div class="space-y-6">
        <div>
          <h2 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Send CHR
          </h2>
          <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Transfer CHR to another address on the Chiral Network
          </p>
        </div>

        <!-- Network Status -->
        <div class="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-800 rounded-lg">
          <div class="flex items-center space-x-2">
            <div class="w-2 h-2 rounded-full {
              networkStatus?.chain_id === 98765 ? 'bg-green-500 animate-pulse' : 'bg-red-500'
            }"></div>
            <span class="text-sm text-gray-600 dark:text-gray-400">
              {#if networkStatus}
                {networkStatus.chain_id === 98765
                  ? `Connected • Block ${networkStatus.latest_block}`
                  : `Wrong network (Chain ID: ${networkStatus.chain_id})`}
              {:else}
                Connecting...
              {/if}
            </span>
          </div>
          {#if networkStatus?.is_syncing}
            <Badge class="bg-yellow-100 text-yellow-800">Syncing</Badge>
          {/if}
        </div>

        <!-- Recipient Address -->
        <div class="space-y-2">
          <Label for="recipient">Recipient Address</Label>
          <Input
            id="recipient"
            bind:value={recipientAddress}
            placeholder="0x..."
            class={validationErrors.recipient ? 'border-red-500' : ''}
            disabled={isProcessing}
          />
          {#if validationErrors.recipient}
            <div class="flex items-center space-x-1 text-xs text-red-600 dark:text-red-400">
              <AlertCircle class="w-3 h-3" />
              <span>{validationErrors.recipient}</span>
            </div>
          {/if}
        </div>

        <!-- Amount -->
        <div class="space-y-2">
          <Label for="amount">Amount (CHR)</Label>
          <Input
            id="amount"
            type="number"
            step="0.000001"
            min="0"
            max={$wallet.balance}
            bind:value={sendAmount}
            placeholder="0.00"
            class={validationErrors.amount ? 'border-red-500' : ''}
            disabled={isProcessing}
          />
          <div class="flex justify-between text-xs">
            <span class="text-gray-500 dark:text-gray-400">
              Balance: {$wallet.balance.toFixed(6)} CHR
            </span>
            {#if validationErrors.amount}
              <span class="text-red-600 dark:text-red-400">
                {validationErrors.amount}
              </span>
            {/if}
          </div>
        </div>

        <!-- Description -->
        <div class="space-y-2">
          <Label for="description">Description (Optional)</Label>
          <Input
            id="description"
            bind:value={description}
            placeholder="Payment for..."
            disabled={isProcessing}
          />
        </div>

        <!-- Errors -->
        {#if validationErrors.network}
          <div class="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
            <div class="flex items-center space-x-2">
              <AlertCircle class="w-4 h-4 text-red-600 dark:text-red-400" />
              <span class="text-sm text-red-600 dark:text-red-400">
                {validationErrors.network}
              </span>
            </div>
          </div>
        {/if}

        <!-- Continue Button -->
        <Button
          on:click={proceedToReview}
          disabled={!recipientAddress || !sendAmount || isProcessing}
          class="w-full"
        >
          Continue to Review
        </Button>
      </div>
    </Card>

  {:else if formStep === 'review'}
    <!-- Review & Gas Selection -->
    <Card class="p-6">
      <div class="space-y-6">
        <div class="flex items-center justify-between">
          <div>
            <h2 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
              Review Transaction
            </h2>
            <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Confirm details and select gas price
            </p>
          </div>
          <Button
            variant="ghost"
            size="sm"
            on:click={backToInput}
            disabled={isProcessing}
          >
            <ArrowLeft class="w-4 h-4 mr-1" />
            Back
          </Button>
        </div>

        <!-- Transaction Details -->
        <div class="p-4 bg-gradient-to-r from-blue-50 to-indigo-50 dark:from-blue-900/20 dark:to-indigo-900/20 rounded-lg space-y-3 border border-blue-200 dark:border-blue-800">
          <div class="flex justify-between">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">From:</span>
            <code class="text-sm font-mono">
              {$etcAccount?.address.slice(0, 10)}...{$etcAccount?.address.slice(-8)}
            </code>
          </div>
          <div class="flex justify-between">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">To:</span>
            <code class="text-sm font-mono">
              {recipientAddress.slice(0, 10)}...{recipientAddress.slice(-8)}
            </code>
          </div>
          <div class="flex justify-between">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Amount:</span>
            <span class="text-lg font-bold">{parseFloat(sendAmount).toFixed(6)} CHR</span>
          </div>
          {#if currentNonce !== null}
            <div class="flex justify-between">
              <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Nonce:</span>
              <span class="text-sm font-mono">{currentNonce}</span>
            </div>
          {/if}
        </div>

        <!-- Gas Estimator -->
        <GasEstimator
          from={$etcAccount?.address || ''}
          to={recipientAddress}
          value={sendAmount}
          on:gasSelected={handleGasSelected}
        />

        <!-- Total Cost Summary -->
        {#if hasGasSelection}
          <div class="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
            <div class="space-y-2">
              <div class="flex justify-between text-sm">
                <span class="text-gray-600 dark:text-gray-400">Amount to send:</span>
                <span class="font-medium">{parseFloat(sendAmount).toFixed(6)} CHR</span>
              </div>
              <div class="flex justify-between text-sm">
                <span class="text-gray-600 dark:text-gray-400">Gas fee ({selectedSpeed}):</span>
                <span class="font-medium">{totalGasCostEth} CHR</span>
              </div>
              <div class="pt-2 border-t border-gray-200 dark:border-gray-600">
                <div class="flex justify-between">
                  <span class="font-semibold">Total:</span>
                  <span class="text-lg font-bold {
                    totalCostEth > $wallet.balance ? 'text-red-600' : 'text-gray-900 dark:text-gray-100'
                  }">
                    {totalCostEth.toFixed(6)} CHR
                  </span>
                </div>
              </div>
            </div>
          </div>
        {/if}

        <!-- Action Buttons -->
        <div class="flex space-x-3">
          <Button
            variant="outline"
            on:click={backToInput}
            disabled={isProcessing}
            class="flex-1"
          >
            Cancel
          </Button>
          <Button
            on:click={executeTransaction}
            disabled={!hasGasSelection || !canAffordTotal || isProcessing}
            class="flex-1"
          >
            <Send class="w-4 h-4 mr-2" />
            Send Transaction
          </Button>
        </div>
      </div>
    </Card>

  {:else if formStep === 'signing'}
    <!-- Signing State -->
    <Card class="p-6">
      <div class="text-center space-y-4 py-8">
        <div class="w-16 h-16 mx-auto bg-blue-100 dark:bg-blue-900/20 rounded-full flex items-center justify-center">
          <Loader class="w-8 h-8 animate-spin text-blue-600" />
        </div>
        <h3 class="text-xl font-semibold">Signing Transaction</h3>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Preparing your transaction for the network...
        </p>
      </div>
    </Card>

  {:else if formStep === 'broadcasting'}
    <!-- Broadcasting State -->
    <Card class="p-6">
      <div class="text-center space-y-4 py-8">
        <div class="w-16 h-16 mx-auto bg-yellow-100 dark:bg-yellow-900/20 rounded-full flex items-center justify-center">
          <Loader class="w-8 h-8 animate-spin text-yellow-600" />
        </div>
        <h3 class="text-xl font-semibold">Broadcasting Transaction</h3>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Submitting to the Chiral Network...
        </p>
      </div>
    </Card>

  {:else if formStep === 'success'}
    <!-- Success State -->
    <Card class="p-6">
      <div class="text-center space-y-6 py-8">
        <div class="w-16 h-16 mx-auto bg-green-100 dark:bg-green-900/20 rounded-full flex items-center justify-center">
          <CheckCircle class="w-8 h-8 text-green-600" />
        </div>
        <div>
          <h3 class="text-xl font-semibold">Transaction Submitted!</h3>
          <p class="text-sm text-gray-500 dark:text-gray-400 mt-2">
            Your transaction is being processed by the network
          </p>
        </div>

        {#if transactionHash}
          <div class="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
            <p class="text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
              Transaction Hash:
            </p>
            <code class="text-xs font-mono text-gray-900 dark:text-gray-100 break-all block">
              {transactionHash}
            </code>
          </div>
        {/if}

        <div class="space-y-3">
          <Button on:click={resetForm} class="w-full">
            <Send class="w-4 h-4 mr-2" />
            Send Another Transaction
          </Button>
          <p class="text-xs text-gray-500 dark:text-gray-400">
            Track this transaction in your history below
          </p>
        </div>
      </div>
    </Card>
  {/if}
</div>
