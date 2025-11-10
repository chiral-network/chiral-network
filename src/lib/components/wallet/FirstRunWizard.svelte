<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import MnemonicWizard from './MnemonicWizard.svelte'
  import { etcAccount, wallet, miningState } from '$lib/stores'
  import { showToast } from '$lib/toast'
  import { t } from 'svelte-i18n'

  export let onComplete: () => void

  let showMnemonicWizard = false
  let mode: 'welcome' | 'mnemonic' = 'welcome'

  function handleCreateNewWallet() {
    mode = 'mnemonic'
    showMnemonicWizard = true
  }

  async function handleMnemonicComplete(ev: { mnemonic: string, passphrase: string, account: { address: string, privateKeyHex: string, index: number, change: number }, name?: string }) {
    try {
      // Set as active account
      etcAccount.set({ address: ev.account.address, private_key: '0x' + ev.account.privateKeyHex })
      wallet.update(w => ({ ...w, address: ev.account.address, balance: 0 }))

      // Reset mining state for new account
      miningState.update(state => ({
        ...state,
        totalRewards: 0,
        blocksFound: 0,
        recentBlocks: []
      }))

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

  async function handleCreateTestWallet() {
    try {
      // Generate a random test address (this is NOT secure, just for testing)
      const randomBytes = new Uint8Array(20)
      crypto.getRandomValues(randomBytes)
      const testAddress = '0x' + Array.from(randomBytes).map(b => b.toString(16).padStart(2, '0')).join('')
      const testPrivateKey = '0x' + Array.from(new Uint8Array(32)).map(() => Math.floor(Math.random() * 256).toString(16).padStart(2, '0')).join('')

      // Set as active account
      etcAccount.set({ address: testAddress, private_key: testPrivateKey })
      wallet.update(w => ({ ...w, address: testAddress, balance: 10 }))

      // Reset mining state for new account
      miningState.update(state => ({
        ...state,
        totalRewards: 10,
        blocksFound: 5,
        recentBlocks: []
      }))

      // Mark first-run as complete
      localStorage.setItem('chiral_first_run_complete', 'true')

      showToast('⚠️ TEST WALLET CREATED - DO NOT USE FOR REAL FUNDS!', 'success')

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
            ⚠️ Create Test Wallet (10 Chiral) - DO NOT USE FOR REAL FUNDS
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
