<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { PaymentCheckpointEvent } from '$lib/services/paymentCheckpointService';
  import Modal from '$lib/components/Modal.svelte';
  import { wallet } from '$lib/stores';
  import { paymentService } from '$lib/services/paymentService';
  import { toHumanReadableSize, formatChiral } from '$lib/utils';

  export let checkpointEvent: PaymentCheckpointEvent | null = null;
  export let fileName: string = '';
  export let showModal: boolean = false;

  const dispatch = createEventDispatcher<{
    pay: { transactionHash: string; amount: number };
    cancel: void;
    close: void;
  }>();

  let processing = false;
  let error: string = '';
  let paymentMode: 'incremental' | 'remaining' | 'upfront' = 'incremental';

  $: availableBalance = $wallet?.balance || 0;

  $: incrementalAmount = checkpointEvent?.amountChiral || 0;
  $: canAffordIncremental = availableBalance >= incrementalAmount;

  // Calculate remaining cost (rough estimate: total file - already transferred)
  $: estimatedRemainingMb = checkpointEvent
    ? ((checkpointEvent.bytesTransferred || 0) / (1024 * 1024))
    : 0;
  $: estimatedRemainingCost = checkpointEvent
    ? estimatedRemainingMb * 0.001 // Rough estimate using price per MB
    : 0;

  async function handlePayIncremental() {
    if (!checkpointEvent) return;

    processing = true;
    error = '';

    try {
      // Process payment for this checkpoint only
      const result = await paymentService.processDownloadPayment(
        checkpointEvent.fileHash,
        fileName,
        checkpointEvent.checkpointMb * 1024 * 1024, // Convert MB to bytes
        checkpointEvent.seederAddress,
        checkpointEvent.seederPeerId
      );

      if (result.success && result.transactionHash) {
        dispatch('pay', {
          transactionHash: result.transactionHash,
          amount: incrementalAmount,
        });
        showModal = false;
      } else {
        error = result.error || 'Payment failed';
      }
    } catch (err) {
      error = err instanceof Error ? err.message : 'Payment failed';
    } finally {
      processing = false;
    }
  }

  async function handlePayRemaining() {
    if (!checkpointEvent) return;

    processing = true;
    error = '';

    try {
      // Process payment for remaining file (estimated)
      const result = await paymentService.processDownloadPayment(
        checkpointEvent.fileHash,
        fileName,
        estimatedRemainingMb * 1024 * 1024,
        checkpointEvent.seederAddress,
        checkpointEvent.seederPeerId
      );

      if (result.success && result.transactionHash) {
        dispatch('pay', {
          transactionHash: result.transactionHash,
          amount: estimatedRemainingCost,
        });
        showModal = false;
      } else {
        error = result.error || 'Payment failed';
      }
    } catch (err) {
      error = err instanceof Error ? err.message : 'Payment failed';
    } finally {
      processing = false;
    }
  }

  function handleCancel() {
    dispatch('cancel');
    showModal = false;
  }

  function handleClose() {
    dispatch('close');
    showModal = false;
  }

</script>

<Modal bind:showModal on:close={handleClose}>
  <div class="p-6 space-y-4">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <h2 class="text-xl font-bold">Payment Checkpoint Reached</h2>
      <button
        onclick={handleClose}
        class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
      >
        ✕
      </button>
    </div>

    {#if checkpointEvent}
      <!-- File Info -->
      <div class="bg-gray-100 dark:bg-gray-800 rounded-lg p-4 space-y-2">
        <div class="flex justify-between">
          <span class="text-sm text-gray-600 dark:text-gray-400">File:</span>
          <span class="text-sm font-medium truncate ml-2" title={fileName}>{fileName}</span>
        </div>
        <div class="flex justify-between">
          <span class="text-sm text-gray-600 dark:text-gray-400">Downloaded:</span>
          <span class="text-sm font-medium">{toHumanReadableSize(checkpointEvent.bytesTransferred)}</span>
        </div>
        <div class="flex justify-between">
          <span class="text-sm text-gray-600 dark:text-gray-400">Checkpoint:</span>
          <span class="text-sm font-medium">{checkpointEvent.checkpointMb} MB</span>
        </div>
      </div>

      <!-- Payment Options -->
      <div class="space-y-3">
        <h3 class="font-semibold text-sm">Payment Required to Continue</h3>

        <!-- Incremental Payment (Default) -->
        <div
          class="border-2 rounded-lg p-4 cursor-pointer transition-all {paymentMode === 'incremental'
            ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
            : 'border-gray-300 dark:border-gray-600 hover:border-gray-400'}"
          onclick={() => (paymentMode = 'incremental')}
          onkeydown={(e) => e.key === 'Enter' && (paymentMode = 'incremental')}
          role="button"
          tabindex="0"
        >
          <div class="flex items-center justify-between">
            <div>
              <div class="font-semibold">Pay for This Checkpoint</div>
              <div class="text-xs text-gray-600 dark:text-gray-400">
                Recommended for building trust incrementally
              </div>
            </div>
            <div class="text-right">
              <div class="text-lg font-bold text-blue-600 dark:text-blue-400">
                {formatChiral(incrementalAmount)} Chiral
              </div>
              <div class="text-xs text-gray-600 dark:text-gray-400">
                {checkpointEvent.checkpointMb} MB
              </div>
            </div>
          </div>
        </div>

        <!-- Pay for Remaining (Optional) -->
        <div
          class="border-2 rounded-lg p-4 cursor-pointer transition-all {paymentMode === 'remaining'
            ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
            : 'border-gray-300 dark:border-gray-600 hover:border-gray-400'}"
          onclick={() => (paymentMode = 'remaining')}
          onkeydown={(e) => e.key === 'Enter' && (paymentMode = 'remaining')}
          role="button"
          tabindex="0"
        >
          <div class="flex items-center justify-between">
            <div>
              <div class="font-semibold">Pay for Remaining File</div>
              <div class="text-xs text-gray-600 dark:text-gray-400">
                Skip future checkpoints (trusted seeders)
              </div>
            </div>
            <div class="text-right">
              <div class="text-lg font-bold">~{formatChiral(estimatedRemainingCost)} Chiral</div>
              <div class="text-xs text-gray-600 dark:text-gray-400">Estimated</div>
            </div>
          </div>
        </div>
      </div>

      <!-- Balance Info -->
      <div class="bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded-lg p-3">
        <div class="flex justify-between text-sm">
          <span class="text-yellow-800 dark:text-yellow-200">Available Balance:</span>
          <span class="font-semibold text-yellow-900 dark:text-yellow-100"
            >{formatChiral(availableBalance)} Chiral</span
          >
        </div>
        {#if !canAffordIncremental}
          <div class="mt-2 text-xs text-red-600 dark:text-red-400">
            ⚠️ Insufficient balance for checkpoint payment
          </div>
        {/if}
      </div>

      <!-- Error Display -->
      {#if error}
        <div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3">
          <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
        </div>
      {/if}

      <!-- Action Buttons -->
      <div class="flex gap-3 pt-2">
        <button
          onclick={handleCancel}
          disabled={processing}
          class="flex-1 px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          Cancel Download
        </button>

        {#if paymentMode === 'incremental'}
          <button
            onclick={handlePayIncremental}
            disabled={processing || !canAffordIncremental}
            class="flex-1 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors font-semibold"
          >
            {#if processing}
              Processing...
            {:else}
              Pay {formatChiral(incrementalAmount)} Chiral
            {/if}
          </button>
        {:else}
          <button
            onclick={handlePayRemaining}
            disabled={processing || availableBalance < estimatedRemainingCost}
            class="flex-1 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors font-semibold"
          >
            {#if processing}
              Processing...
            {:else}
              Pay ~{formatChiral(estimatedRemainingCost)} Chiral
            {/if}
          </button>
        {/if}
      </div>

      <!-- Info Note -->
      <div class="text-xs text-gray-500 dark:text-gray-400 text-center">
        💡 Download will resume automatically after payment is confirmed
      </div>
    {/if}
  </div>
</Modal>
