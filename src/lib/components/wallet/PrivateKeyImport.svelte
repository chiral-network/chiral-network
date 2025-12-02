<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { validatePrivateKeyFormat } from '$lib/utils/validation'
  import { showToast } from '$lib/toast'
  import { t } from 'svelte-i18n'

  type TranslateParams = { values?: Record<string, unknown>; default?: string }
  const tr = (key: string, params?: TranslateParams) => $t(key, params)

  export let onComplete: (args: { address: string, privateKeyHex: string }) => void
  export let onCancel: () => void

  let privateKey = ''
  let error = ''
  let isBusy = false
  let privateKeyVisible = false

  async function importPrivateKey() {
    try {
      isBusy = true
      error = ''

      // Validate private key format
      const validation = validatePrivateKeyFormat(privateKey)
      if (!validation.isValid) {
        error = validation.error || 'Invalid private key format'
        isBusy = false
        return
      }

      // Normalize private key (add 0x prefix if missing)
      const trimmed = privateKey.trim()
      const privateKeyWithPrefix = trimmed.startsWith('0x') ? trimmed : '0x' + trimmed

      // Use walletService to properly import account
      const { walletService } = await import('$lib/wallet')
      const result = await walletService.importAccount(privateKeyWithPrefix)

      // Check if Geth is running to show appropriate message
      const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
      let gethRunning = false
      if (isTauri) {
        const { invoke } = await import('@tauri-apps/api/core')
        try {
          gethRunning = await invoke<boolean>('is_geth_running')
        } catch (e) {
          console.warn('Could not check Geth status:', e)
        }
      }

      // Complete with address and private key from backend
      onComplete({
        address: result.address,
        privateKeyHex: result.private_key
      })

      // Show message based on Geth status
      if (gethRunning) {
        showToast(tr('toasts.wallet.privateKey.imported'), 'success')
      } else {
        showToast('Wallet imported! Start the Chiral node on the Network page to load your balance.', 'success')
      }
    } catch (e) {
      console.error('Failed to import private key:', e)
      error = String(e)
    } finally {
      isBusy = false
    }
  }

  function handleKeyDown(event: CustomEvent<any> & { key?: string }) {
    const keyboardEvent = event as unknown as KeyboardEvent
    if (keyboardEvent.key === 'Enter' && !isBusy && privateKey.trim()) {
      importPrivateKey()
    }
  }
</script>

<div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4">
  <Card class="w-full max-w-2xl p-6 space-y-4">
    <h2 class="text-xl font-semibold">{tr('account.privateKeyImport.title')}</h2>
    <p class="text-sm text-muted-foreground">
      {tr('account.privateKeyImport.description')}
    </p>

    <div class="space-y-3">
      <Label>{tr('account.privateKeyImport.privateKeyLabel')}</Label>
      <div class="relative">
        <Input
          type={privateKeyVisible ? 'text' : 'password'}
          bind:value={privateKey}
          placeholder={tr('account.privateKeyImport.privateKeyPlaceholder')}
          on:keydown={handleKeyDown}
          class="pr-10"
        />
        <button
          type="button"
          class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
          on:click={() => privateKeyVisible = !privateKeyVisible}
        >
          {privateKeyVisible ? '👁️' : '👁️‍🗨️'}
        </button>
      </div>
      <p class="text-xs text-muted-foreground">
        {tr('account.privateKeyImport.hint')}
      </p>
    </div>

    <div class="p-3 rounded-md bg-amber-500/10 border border-amber-500/50">
      <p class="text-sm text-amber-600 dark:text-amber-400">
        ⚠️ {tr('account.privateKeyImport.warning')}
      </p>
    </div>

    {#if error}
      <p class="text-sm text-red-500">{error}</p>
    {/if}

    <div class="flex gap-2 justify-end">
      <Button variant="outline" on:click={onCancel} disabled={isBusy}>
        {tr('common.cancel')}
      </Button>
      <Button
        on:click={importPrivateKey}
        disabled={isBusy || !privateKey.trim()}
      >
        {isBusy ? tr('account.privateKeyImport.importing') : tr('account.privateKeyImport.import')}
      </Button>
    </div>
  </Card>
</div>

<style>
  /* Ensure input background respects theme */
</style>
