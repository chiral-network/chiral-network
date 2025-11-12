<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import { AlertCircle, X } from 'lucide-svelte'
  import { t } from 'svelte-i18n'
  import { goto } from '@mateothegreat/svelte5-router'

  export let onDismiss: () => void
  export let onCreateWallet: () => void
</script>

<div class="bg-amber-500/10 border border-amber-500/30 rounded-lg p-4 mb-4">
  <div class="flex items-start gap-3">
    <AlertCircle class="h-5 w-5 text-amber-500 flex-shrink-0 mt-0.5" />
    <div class="flex-1 space-y-2">
      <div class="flex items-start justify-between">
        <div>
          <h3 class="font-semibold text-amber-900 dark:text-amber-100">
            {$t('mining.temporaryAccount.title')}
          </h3>
          <p class="text-sm text-amber-800 dark:text-amber-200 mt-1">
            {$t('mining.temporaryAccount.description')}
          </p>
        </div>
        <button
          on:click={onDismiss}
          class="text-amber-700 hover:text-amber-900 dark:text-amber-300 dark:hover:text-amber-100"
        >
          <X class="h-4 w-4" />
        </button>
      </div>

      <div class="flex flex-wrap gap-2 mt-3">
        <Button size="sm" on:click={onCreateWallet}>
          {$t('mining.temporaryAccount.createWallet')}
        </Button>
        <Button size="sm" variant="outline" on:click={() => goto('/account')}>
          {$t('mining.temporaryAccount.goToAccount')}
        </Button>
      </div>
    </div>
  </div>
</div>
