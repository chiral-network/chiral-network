<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import MnemonicWizard from './MnemonicWizard.svelte'
  import { etcAccount, wallet, miningState } from '$lib/stores'
  import { showToast } from '$lib/toast'
  import { t } from 'svelte-i18n'
  import { onMount } from 'svelte'

  export let onComplete: () => void

  let showMnemonicWizard = false
  let mode: 'welcome' | 'mnemonic' = 'welcome'

  onMount(() => {
    // Wizard initialization
  })

  function handleCreateNewWallet() {
    mode = 'mnemonic'
    showMnemonicWizard = true
  }

  async function handleMnemonicComplete(ev: { mnemonic: string, passphrase: string, account: { address: string, privateKeyHex: string, index: number, change: number }, name?: string }) {
    try {
      // Import to backend to set as active account
      const { invoke } = await import('@tauri-apps/api/core')
      const privateKeyWithPrefix = '0x' + ev.account.privateKeyHex
      
      await invoke('import_chiral_account', { privateKey: privateKeyWithPrefix })
      
      // Set frontend account (backend is now also set)
      etcAccount.set({ address: ev.account.address, private_key: privateKeyWithPrefix })
      wallet.update(w => ({ ...w, address: ev.account.address, balance: 0 }))

      // Reset mining state for new account
      miningState.update(state => ({
        ...state,
        totalRewards: 0,
        blocksFound: 0,
        recentBlocks: []
      }))

      // Encourage saving to keystore (optional - user can do later)
      showToast($t('account.firstRun.accountCreated'), 'success')

      onComplete()
    } catch (error) {
      console.error('Failed to complete first-run setup:', error)
      showToast($t('account.firstRun.error'), 'error')
    }
  }

  async function handleCreateTestWallet() {
    try {
      // Import walletService for backend integration
      const { walletService } = await import('$lib/wallet')
      
      // Create a regular account through backend
      await walletService.createAccount()

      showToast('Test wallet "TestWallet" created!', 'success')

      onComplete()
    } catch (error) {
      console.error('Failed to create test wallet:', error)
      showToast('Failed to create test wallet', 'error')
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
          
          <div class="relative">
            <div class="absolute inset-0 flex items-center">
              <span class="w-full border-t border-muted"></span>
            </div>
            <div class="relative flex justify-center text-xs uppercase">
              <span class="bg-background px-2 text-muted-foreground">For Testing Only</span>
            </div>
          </div>

          <Button 
            on:click={handleCreateTestWallet} 
            variant="outline" 
            class="w-full py-4 border-amber-500/50 text-amber-600 dark:text-amber-400 hover:bg-amber-500/10"
          >
            ⚠️ Create Test Wallet - For Testing Only
          </Button>
        </div>

        <p class="text-xs text-center text-muted-foreground">
          {$t('account.firstRun.requiresWallet')}
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
