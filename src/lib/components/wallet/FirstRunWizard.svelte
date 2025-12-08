<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import MnemonicWizard from './MnemonicWizard.svelte'
  import { etcAccount, wallet, miningState, transactions } from '$lib/stores'
  import { showToast } from '$lib/toast'
  import { t } from 'svelte-i18n'
  import { onMount } from 'svelte'
  import { validatePrivateKeyFormat } from '$lib/utils/validation'
  import { walletService } from '$lib/wallet'

  export let onComplete: () => void

  let showMnemonicWizard = false
  let mode: 'welcome' | 'mnemonic' | 'import' = 'welcome'
  let importPrivateKey = ''
  let isImportingAccount = false
  let importedSnapshot: any = null

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
      // Create a regular account through backend
      await walletService.createAccount()

      // showToast('Test wallet "TestWallet" created!', 'success')
      showToast($t('toasts.account.firstRun.testWalletCreated'), 'success')
      onComplete()
    } catch (error) {
      console.error('Failed to create test wallet:', error)
      // showToast('Failed to create test wallet', 'error')
      showToast($t('toasts.account.firstRun.testWalletError'), 'error')
    }
  }

  function handleMnemonicCancel() {
    showMnemonicWizard = false
    mode = 'welcome'
  }

  const msg = (key: string, fallback: string) => {
    const val = $t(key);
    return val === key ? fallback : val;
  }

  async function loadPrivateKeyFromFile() {
    try {
      const fileInput = document.createElement('input')
      fileInput.type = 'file'
      fileInput.accept = '.json'
      fileInput.style.display = 'none'

      fileInput.onchange = async (event) => {
        const file = (event.target as HTMLInputElement).files?.[0]
        if (!file) return

        try {
          const fileContent = await file.text()
          const accountData = JSON.parse(fileContent)
          const extractedPrivateKey = accountData.privateKey ?? accountData.private_key

          if (!extractedPrivateKey) {
            showToast(msg('account.firstRun.importFileInvalid', 'Invalid wallet file (missing private key)'), 'error')
            return
          }

          importPrivateKey = extractedPrivateKey
          importedSnapshot = accountData
          // If the export contains prior balance/tx info, hydrate UI immediately
          if (typeof accountData.balance === 'number') {
            wallet.update(w => ({ ...w, balance: accountData.balance, actualBalance: accountData.balance }))
          }
          if (Array.isArray(accountData.transactions)) {
            const hydrated = accountData.transactions.map((tx: any) => ({
              ...tx,
              date: tx.date ? new Date(tx.date) : new Date()
            }))
            transactions.set(hydrated)
          }

          showToast(msg('account.firstRun.importFileLoaded', 'Wallet file loaded. Ready to import.'), 'success')
        } catch (error) {
          console.error('Error reading wallet file', error)
          showToast(msg('account.firstRun.importFileError', `Error reading wallet file: ${String(error)}`), 'error')
        }
      }

      document.body.appendChild(fileInput)
      fileInput.click()
      document.body.removeChild(fileInput)
    } catch (error) {
      console.error('Error loading wallet file', error)
      showToast($t('account.firstRun.importFileError') ?? `Error loading file: ${String(error)}`, 'error')
    }
  }

  async function handleImportExistingWallet() {
    if (!importPrivateKey.trim()) {
      showToast($t('account.firstRun.importMissingKey') ?? 'Enter your private key first', 'error')
      return
    }

    const validation = validatePrivateKeyFormat(importPrivateKey)
    if (!validation.isValid) {
      showToast(validation.error ?? 'Invalid private key format', 'error')
      return
    }

    isImportingAccount = true
    try {
      const normalized = importPrivateKey.trim().startsWith('0x')
        ? importPrivateKey.trim()
        : `0x${importPrivateKey.trim()}`
      const account = await walletService.importAccount(normalized)
      // If we loaded a snapshot from file, hydrate wallet/txs immediately
      if (importedSnapshot) {
        if (typeof importedSnapshot.balance === 'number') {
          wallet.update(w => ({ ...w, balance: importedSnapshot.balance, actualBalance: importedSnapshot.balance }))
        }
        if (Array.isArray(importedSnapshot.transactions)) {
          const hydrated = importedSnapshot.transactions.map((tx: any) => ({
            ...tx,
            date: tx.date ? new Date(tx.date) : new Date()
          }))
          transactions.set(hydrated)
        }
      }
      wallet.update(w => ({
        ...w,
        address: account.address,
        pendingTransactions: 0
      }))
      // Mirror the keystore load flow: refresh transactions/balance and start progressive loading
      await walletService.refreshTransactions()
      await walletService.refreshBalance()
      walletService.startProgressiveLoading()
      importPrivateKey = ''
      importedSnapshot = null
      showToast(msg('account.firstRun.importSuccess', 'Wallet imported successfully'), 'success')
      mode = 'welcome'
      onComplete()
    } catch (error) {
      console.error('Failed to import wallet', error)
      showToast(msg('account.firstRun.importError', 'Failed to import wallet'), 'error')
    } finally {
      isImportingAccount = false
    }
  }
</script>

{#if mode === 'welcome'}
  <div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4">
    <Card class="w-full max-w-3xl p-8 space-y-6">
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

          <Button on:click={() => mode = 'import'} variant="outline" class="w-full py-6 text-lg">
            {$t('account.firstRun.importWallet') === 'account.firstRun.importWallet'
                ? 'Import Existing Wallet'
                : $t('account.firstRun.importWallet')}
          </Button>
          
          {#if import.meta.env.DEV}
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
              Create Test Wallet
            </Button>
          {/if}
        </div>

        <p class="text-sm text-center text-muted-foreground">
          {$t('account.firstRun.requiresWallet')}
        </p>
      </div>
    </Card>
  </div>
{/if}

{#if mode === 'import'}
  <div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4">
    <Card class="w-full max-w-2xl p-8 space-y-6">
      <div class="space-y-2">
        <h2 class="text-3xl font-bold text-center">
          {$t('account.firstRun.importTitle') === 'account.firstRun.importTitle'
            ? 'Import Existing Wallet'
            : $t('account.firstRun.importTitle')}
        </h2>
        <p class="text-center text-muted-foreground">
          {$t('account.firstRun.importDescription') === 'account.firstRun.importDescription'
            ? 'Paste your private key or load an exported wallet file to restore access.'
            : $t('account.firstRun.importDescription')}
        </p>
      </div>

      <div class="space-y-4">
        <div class="flex flex-col gap-2">
          <Input
            class="flex-1"
            placeholder="0x..."
            bind:value={importPrivateKey}
            autocomplete="off"
            spellcheck="false"
          />
          <Button variant="outline" on:click={loadPrivateKeyFromFile}>
            {$t('account.firstRun.loadFromFile') === 'account.firstRun.loadFromFile'
              ? 'Load from file'
              : $t('account.firstRun.loadFromFile')}
          </Button>
        </div>

        <Button
          class="w-full"
          on:click={handleImportExistingWallet}
          disabled={!importPrivateKey || isImportingAccount}
        >
          {isImportingAccount
            ? ($t('account.firstRun.importing') === 'account.firstRun.importing'
              ? 'Importing...'
              : $t('account.firstRun.importing'))
            : ($t('account.firstRun.importWallet') === 'account.firstRun.importWallet'
              ? 'Import Wallet'
              : $t('account.firstRun.importWallet'))}
        </Button>

        <button
          class="block w-full text-center text-xs font-semibold text-primary underline underline-offset-4 decoration-dotted px-4 py-2 rounded-full transition-colors hover:text-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          type="button"
          on:click={() => {
            mode = 'welcome'
            importPrivateKey = ''
          }}
        >
          {$t('account.firstRun.backToCreate') === 'account.firstRun.backToCreate'
            ? 'Back to Welcome'
            : $t('account.firstRun.backToCreate')}
        </button>
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

<style>
</style>
