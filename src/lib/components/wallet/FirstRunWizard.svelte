<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import MnemonicWizard from './MnemonicWizard.svelte'
  import { etcAccount, wallet } from '$lib/stores'
  import { invoke } from '@tauri-apps/api/core'
  import { showToast } from '$lib/toast'
  import { t } from 'svelte-i18n'

  export let onComplete: () => void
  export let onSkip: () => void

  let showMnemonicWizard = false
  let mode: 'welcome' | 'mnemonic' = 'welcome'

  function handleCreateNewWallet() {
    mode = 'mnemonic'
    showMnemonicWizard = true
  }

  function handleSkipForNow() {
    // Set flag that user skipped first-run
    localStorage.setItem('chiral_first_run_skipped', 'true')
    showToast($t('account.firstRun.skipped'), 'info')
    onSkip()
  }

  async function handleMnemonicComplete(ev: { mnemonic: string, passphrase: string, account: { address: string, privateKeyHex: string, index: number, change: number }, name?: string }) {
    try {
      // Set as active account
      etcAccount.set({ address: ev.account.address, private_key: '0x' + ev.account.privateKeyHex })
      wallet.update(w => ({ ...w, address: ev.account.address }))

      // Mark first-run as complete
      localStorage.setItem('chiral_first_run_complete', 'true')

      // Encourage saving to keystore (optional - user can do later)
      showToast($t('account.firstRun.accountCreated'), 'success')

      onComplete()
    } catch (error) {
      console.error('Failed to complete first-run setup:', error)
      showToast($t('account.firstRun.error'), 'error')
    }
  }

  function handleMnemonicCancel() {
    showMnemonicWizard = false
    mode = 'welcome'
  }
</script>

{#if mode === 'welcome'}
  <div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4">
    <Card class="w-full max-w-2xl p-8 space-y-6">
      <div class="space-y-2">
        <h2 class="text-3xl font-bold text-center">{$t('account.firstRun.welcome')}</h2>
        <p class="text-center text-muted-foreground">
          {$t('account.firstRun.description')}
        </p>
      </div>

      <div class="space-y-4">
        <div class="p-4 border rounded-lg space-y-2">
          <h3 class="font-semibold text-lg">{$t('account.firstRun.whyAccount')}</h3>
          <ul class="list-disc list-inside space-y-1 text-sm text-muted-foreground">
            <li>{$t('account.firstRun.reason1')}</li>
            <li>{$t('account.firstRun.reason2')}</li>
            <li>{$t('account.firstRun.reason3')}</li>
          </ul>
        </div>

        <div class="flex flex-col gap-3">
          <Button on:click={handleCreateNewWallet} class="w-full py-6 text-lg">
            {$t('account.firstRun.createWallet')}
          </Button>

          <Button on:click={handleSkipForNow} variant="outline" class="w-full">
            {$t('account.firstRun.skipForNow')}
          </Button>
        </div>

        <p class="text-xs text-center text-muted-foreground">
          {$t('account.firstRun.skipWarning')}
        </p>
      </div>
    </Card>
  </div>
{/if}

{#if showMnemonicWizard}
  <MnemonicWizard
    mode="create"
    onComplete={handleMnemonicComplete}
    onCancel={handleMnemonicCancel}
  />
{/if}
