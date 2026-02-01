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
  import { getCachedBalance, setCachedBalance, formatRelativeTime } from '$lib/utils/keystoreBalanceCache'
  import { getWalletName, setWalletName, removeWalletName } from '$lib/utils/walletNameCache'
  import { saveWalletMetadata, touchWalletLastUsed } from '$lib/utils/walletCache'

  export let onComplete: () => void

  let showMnemonicWizard = false
  let mode: 'welcome' | 'mnemonic' | 'import' = 'welcome'
  let importPrivateKey = ''
  let importWalletName = ''
  let importWalletPassword = ''
  let isImportingAccount = false
  let importedSnapshot: any = null
  let showMnemonicRecovery = false

  // Keystore account management
  let keystoreAccounts: string[] = []
  let keystoreBalances = new Map<string, { balance: string; timestamp: number }>()
  let keystoreNames = new Map<string, string>()
  let loadingKeystoreAccounts = false
  let isUnlockingAccount = false

  let pendingKeystoreAddress: string | null = null
  let keystorePasswordInput = ''
  let keystorePasswordError = ''
  let showKeystorePasswordPrompt = false
  let pendingDeleteAddress: string | null = null
  let showDeleteConfirm = false

  onMount(async () => {
    // Load keystore accounts on wizard open
    await loadKeystoreAccounts()
  })

  function handleCreateNewWallet() {
    mode = 'mnemonic'
    showMnemonicWizard = true
  }

  async function handleMnemonicComplete(ev: { mnemonic: string, passphrase: string, account: { address: string, privateKeyHex: string, index: number, change: number }, name?: string, password?: string }) {
    try {
      // Import to backend to set as active account
      const { invoke } = await import('@tauri-apps/api/core')
      const privateKeyWithPrefix = '0x' + ev.account.privateKeyHex

      await invoke('import_chiral_account', { privateKey: privateKeyWithPrefix })

      // Set frontend account (backend is now also set)
      etcAccount.set({ address: ev.account.address, private_key: privateKeyWithPrefix })
      wallet.update(w => ({ ...w, address: ev.account.address, balance: 0 }))

      // Save wallet name if provided
      if (ev.name) {
        setWalletName(ev.account.address, ev.name)
      }

      saveWalletMetadata(ev.account.address, {
        name: ev.name,
        source: 'create'
      })

      // Save to keystore with user-provided password (optional)
      try {
        await walletService.saveToKeystore(ev.password ?? '', {
          address: ev.account.address,
          private_key: privateKeyWithPrefix
        })
        console.log('Saved wallet to keystore')
      } catch (error) {
        console.warn('Failed to save to keystore:', error)
      }

      // Reset mining state for new account
      miningState.update(state => ({
        ...state,
        totalRewards: 0,
        blocksFound: 0,
        recentBlocks: []
      }))

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
      const account = await walletService.createAccount()

      try {
        await walletService.saveToKeystore('', {
          address: account.address,
          private_key: account.private_key
        })
      } catch (error) {
        console.warn('Failed to save test wallet to keystore:', error)
      }

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

  // Format address as 0x1234...5678
  function formatAddress(address: string): string {
    return `${address.slice(0, 6)}...${address.slice(-4)}`
  }

  // Get display name for wallet (name or formatted address)
  function getWalletDisplayName(address: string): string {
    const name = keystoreNames.get(address.toLowerCase())
    return name || formatAddress(address)
  }

  // Load keystore accounts from backend
  async function loadKeystoreAccounts() {
    loadingKeystoreAccounts = true

    try {
      // Get list of addresses from keystore
      keystoreAccounts = await walletService.listKeystoreAccounts()
      console.log('Loaded keystore accounts:', keystoreAccounts)

      // Load cached names and balances
      for (const address of keystoreAccounts) {
        // Load name from localStorage
        const name = getWalletName(address)
        if (name) {
          keystoreNames.set(address.toLowerCase(), name)
        }

        // Load cached balance
        const cached = getCachedBalance(address)
        if (cached) {
          keystoreBalances.set(address.toLowerCase(), cached)
        } else {
          keystoreBalances.set(address.toLowerCase(), { balance: '--', timestamp: 0 })
        }
      }

      // Trigger reactivity
      keystoreNames = new Map(keystoreNames)
      keystoreBalances = new Map(keystoreBalances)

      // Async refresh balances in background (don't await)
      if (keystoreAccounts.length > 0) {
        refreshKeystoreBalances()
      }

    } catch (error) {
      console.error('Failed to load keystore accounts:', error)
      showToast(msg('wallet.errors.loadKeystoreFailed', 'Failed to load saved wallets'), 'error')
    } finally {
      loadingKeystoreAccounts = false
    }
  }

  // Refresh balances from blockchain in background
  async function refreshKeystoreBalances() {
    const { invoke } = await import('@tauri-apps/api/core')

    for (const address of keystoreAccounts) {
      try {
        const balance = await invoke<string>('get_account_balance', { address })

        // Update in-memory map
        keystoreBalances.set(address.toLowerCase(), {
          balance,
          timestamp: Date.now()
        })

        // Trigger reactivity
        keystoreBalances = new Map(keystoreBalances)

        // Cache to localStorage
        setCachedBalance(address, balance)

      } catch (error) {
        // Silent failure - Geth may not be running yet
        console.warn(`Could not fetch balance for ${address}:`, error)
      }
    }
  }

  // Load selected keystore account (auto-unlock with default password)
  function isPasswordError(error: unknown): boolean {
    const message = String(error).toLowerCase()
    return message.includes('password') || message.includes('decrypt') || message.includes('keystore')
  }

  async function loadSelectedKeystoreAccount(address: string, password: string, allowPrompt = false): Promise<boolean> {
    isUnlockingAccount = true

    try {
      const account = await walletService.loadFromKeystore(address, password)

      // Update stores
      wallet.update(w => ({
        ...w,
        address: account.address,
        pendingTransactions: 0
      }))

      // Refresh data
      await walletService.refreshTransactions()
      await walletService.refreshBalance()
      walletService.startProgressiveLoading()

      touchWalletLastUsed(account.address)

      touchWalletLastUsed(account.address)

      // Show success and complete wizard
      showToast(msg('wallet.wizard.unlockSuccess', 'Wallet loaded successfully'), 'success')
      onComplete()
      return true

    } catch (error) {
      console.error('Failed to load keystore account:', error)
      if (allowPrompt && isPasswordError(error)) {
        pendingKeystoreAddress = address
        keystorePasswordInput = ''
        keystorePasswordError = ''
        showKeystorePasswordPrompt = true
        return false
      }
      if (showKeystorePasswordPrompt && isPasswordError(error)) {
        keystorePasswordError = msg('wallet.errors.loadFailed', 'Failed to load wallet. It may have been saved with a password.')
        return false
      }
      const errorMsg = msg('wallet.errors.loadFailed', 'Failed to load wallet. It may have been saved with a password.')
      showToast(errorMsg, 'error')
      return false
    } finally {
      isUnlockingAccount = false
    }
  }

  async function confirmKeystorePassword() {
    if (!pendingKeystoreAddress) return
    keystorePasswordError = ''
    const success = await loadSelectedKeystoreAccount(pendingKeystoreAddress, keystorePasswordInput, false)
    if (success) {
      showKeystorePasswordPrompt = false
      pendingKeystoreAddress = null
      keystorePasswordInput = ''
    }
  }

  // Delete wallet from keystore
  function requestDeleteWallet(address: string, event: MouseEvent) {
    event.stopPropagation()
    pendingDeleteAddress = address
    showDeleteConfirm = true
  }

  async function deleteWallet() {
    if (!pendingDeleteAddress) return
    const address = pendingDeleteAddress
    try {
      await walletService.deleteKeystoreAccount(address)

      // Remove from localStorage caches
      removeWalletName(address)

      // Reload the list
      await loadKeystoreAccounts()

      showToast(msg('wallet.wizard.deleteSuccess', 'Wallet deleted successfully'), 'success')
    } catch (error) {
      console.error('Failed to delete wallet:', error)
      showToast(msg('wallet.errors.deleteFailed', 'Failed to delete wallet'), 'error')
    } finally {
      pendingDeleteAddress = null
      showDeleteConfirm = false
    }
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

      // Save wallet name if provided
      if (importWalletName.trim()) {
        setWalletName(account.address, importWalletName.trim())
      }

      saveWalletMetadata(account.address, {
        name: importWalletName.trim() || undefined,
        source: 'import'
      })

      // Save to keystore with user-provided password (optional)
      try {
        await walletService.saveToKeystore(importWalletPassword, {
          address: account.address,
          private_key: normalized
        })
        console.log('Saved imported wallet to keystore')
      } catch (error) {
        console.warn('Failed to save to keystore:', error)
      }

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
      importWalletName = ''  // Clear name input
      importWalletPassword = ''
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

  async function handleMnemonicRecovery(ev: { mnemonic: string, passphrase: string, account: { address: string, privateKeyHex: string, index: number, change: number }, name?: string, password?: string }) {
    try {
      // Import to backend to set as active account
      const { invoke } = await import('@tauri-apps/api/core')
      const privateKeyWithPrefix = '0x' + ev.account.privateKeyHex

      await invoke('import_chiral_account', { privateKey: privateKeyWithPrefix })

      // Set frontend account (backend is now also set)
      etcAccount.set({ address: ev.account.address, private_key: privateKeyWithPrefix })
      wallet.update(w => ({ ...w, address: ev.account.address, balance: 0 }))

      // Save wallet name if provided
      if (ev.name) {
        setWalletName(ev.account.address, ev.name)
      }

      saveWalletMetadata(ev.account.address, {
        name: ev.name,
        source: 'import'
      })

      // Save to keystore with user-provided password (optional)
      try {
        await walletService.saveToKeystore(ev.password ?? '', {
          address: ev.account.address,
          private_key: privateKeyWithPrefix
        })
        console.log('Saved recovered wallet to keystore')
      } catch (error) {
        console.warn('Failed to save to keystore:', error)
      }

      // Reset mining state for recovered account
      miningState.update(state => ({
        ...state,
        totalRewards: 0,
        blocksFound: 0,
        recentBlocks: []
      }))

      // WalletService 동기화 트리거
      await walletService.refreshTransactions()
      await walletService.refreshBalance()
      walletService.startProgressiveLoading()

      showToast(msg('account.firstRun.importSuccess', 'Account recovered from recovery phrase'), 'success')
      showMnemonicRecovery = false
      onComplete()
    } catch (error) {
      console.error('Failed to recover from mnemonic:', error)
      showToast(msg('account.firstRun.importError', `Recovery failed: ${error}`), 'error')
    }
  }
</script>

{#if mode === 'welcome'}
  <div class="fixed inset-0 z-50 bg-background/90 backdrop-blur-lg flex items-center justify-center p-4">
    <Card class="w-full max-w-3xl p-8 space-y-6">
      <div class="space-y-2">
        <h2 class="text-3xl font-bold text-center">{$t('account.firstRun.welcome')}</h2>
        <p class="text-center text-muted-foreground">
          {$t('account.firstRun.description')}
        </p>
      </div>

      <!-- KEYSTORE WALLETS SECTION -->
      {#if keystoreAccounts.length > 0}
        <div class="space-y-4">
          <h3 class="text-lg font-semibold">
            {$t('wallet.wizard.existingWallets') === 'wallet.wizard.existingWallets'
              ? 'Your Saved Wallets'
              : $t('wallet.wizard.existingWallets')}
          </h3>


          {#if loadingKeystoreAccounts}
            <div class="text-center py-4">
              <div class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
              <p class="text-sm text-muted-foreground mt-2">
                {$t('wallet.wizard.loadingWallets') === 'wallet.wizard.loadingWallets'
                  ? 'Loading wallets...'
                  : $t('wallet.wizard.loadingWallets')}
              </p>
            </div>
          {:else}
            <div class="space-y-2 max-h-64 overflow-y-auto">
              {#each keystoreAccounts as address}
                {@const balanceData = keystoreBalances.get(address.toLowerCase())}
                {@const displayName = getWalletDisplayName(address)}

                <Card class="transition-all hover:border-primary/50 cursor-pointer relative group">
                  <button
                    class="w-full text-left p-4"
                    on:click={() => loadSelectedKeystoreAccount(address, '', true)}
                    disabled={isUnlockingAccount}
                    type="button"
                  >
                    <div class="flex justify-between items-center">
                      <div class="flex-1 min-w-0 pr-2">
                        <p class="font-medium text-base mb-1">{displayName}</p>
                        <p class="font-mono text-xs text-muted-foreground mb-2">
                          {formatAddress(address)}
                        </p>
                        <div class="flex items-center gap-2">
                          <span class="text-sm">
                            {$t('wallet.balance') || 'Balance'}:
                          </span>
                          <span class="font-semibold">
                            {balanceData?.balance || '--'} CHRL
                          </span>
                          {#if balanceData && balanceData.timestamp > 0}
                            <span class="text-xs text-muted-foreground">
                              ({formatRelativeTime(balanceData.timestamp)})
                            </span>
                          {/if}
                        </div>
                      </div>

                      {#if isUnlockingAccount}
                        <div class="inline-block animate-spin rounded-full h-5 w-5 border-b-2 border-primary"></div>
                      {:else}
                        <svg class="w-5 h-5 text-muted-foreground" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
                        </svg>
                      {/if}
                    </div>
                  </button>

                  <!-- Delete button (top-right corner) -->
                  <button
                    class="absolute top-2 right-2 p-1.5 rounded-full bg-red-100 hover:bg-red-200 text-red-600 opacity-0 group-hover:opacity-100 transition-opacity duration-200"
                    on:click={(e) => requestDeleteWallet(address, e)}
                    disabled={isUnlockingAccount}
                    type="button"
                    aria-label={$t('wallet.wizard.deleteWallet') === 'wallet.wizard.deleteWallet' ? 'Delete wallet' : $t('wallet.wizard.deleteWallet')}
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                    </svg>
                  </button>
                </Card>
              {/each}
            </div>
          {/if}

          <div class="relative">
            <div class="absolute inset-0 flex items-center">
              <span class="w-full border-t border-muted"></span>
            </div>
            <div class="relative flex justify-center text-xs uppercase">
              <span class="bg-background px-2 text-muted-foreground">
                {$t('wallet.wizard.orCreateImport') === 'wallet.wizard.orCreateImport'
                  ? 'Or'
                  : $t('wallet.wizard.orCreateImport')}
              </span>
            </div>
          </div>
        </div>
      {/if}

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
                <span class="bg-background px-2 text-muted-foreground">{$t('wallet.testWallet.header')}</span>
              </div>
            </div>

            <Button
              on:click={handleCreateTestWallet}
              variant="outline"
              class="w-full py-4 border-amber-500/50 text-amber-600 dark:text-amber-400 hover:bg-amber-500/10"
            >
              {$t('wallet.testWallet.button')}
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
        <!-- Wallet Name Input -->
        <div class="flex flex-col gap-2">
          <label for="import-wallet-name" class="text-sm font-medium">
            {$t('wallet.wizard.walletName') === 'wallet.wizard.walletName'
              ? 'Wallet Name (optional)'
              : $t('wallet.wizard.walletName')}
          </label>
          <Input
            id="import-wallet-name"
            bind:value={importWalletName}
            placeholder={$t('wallet.wizard.walletNamePlaceholder') === 'wallet.wizard.walletNamePlaceholder'
              ? 'e.g., Main Wallet'
              : $t('wallet.wizard.walletNamePlaceholder')}
          />
        </div>

        <!-- Wallet Password Input -->
        <div class="flex flex-col gap-2">
          <label for="import-wallet-password" class="text-sm font-medium">
            {$t('wallet.wizard.passwordOptional') === 'wallet.wizard.passwordOptional'
              ? 'Password (optional)'
              : $t('wallet.wizard.passwordOptional')}
          </label>
          <Input
            id="import-wallet-password"
            type="password"
            bind:value={importWalletPassword}
            placeholder={$t('wallet.wizard.passwordPlaceholder') === 'wallet.wizard.passwordPlaceholder'
              ? 'Leave empty if none'
              : $t('wallet.wizard.passwordPlaceholder')}
          />
        </div>

        <!-- Private Key Input -->
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

        <div class="relative py-2">
          <div class="absolute inset-0 flex items-center">
            <span class="w-full border-t"></span>
          </div>
          <div class="relative flex justify-center text-xs uppercase">
            <span class="bg-background px-2 text-muted-foreground">Or</span>
          </div>
        </div>

        <Button
          class="w-full"
          variant="outline"
          on:click={() => showMnemonicRecovery = true}
          disabled={isImportingAccount}
        >
          Recover from 12-word phrase
        </Button>

        <button
          class="block w-full text-center text-xs font-semibold text-primary underline underline-offset-4 decoration-dotted px-4 py-2 rounded-full transition-colors hover:text-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          type="button"
          on:click={() => {
            mode = 'welcome'
            importPrivateKey = ''
            importWalletPassword = ''
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

<!-- Mnemonic Recovery Modal for Login -->
{#if showMnemonicRecovery}
  <MnemonicWizard
    mode="import"
    onComplete={handleMnemonicRecovery}
    onCancel={() => showMnemonicRecovery = false}
  />
{/if}

{#if showKeystorePasswordPrompt}
  <div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4">
    <Card class="w-full max-w-sm p-6 space-y-4">
      <h3 class="text-lg font-semibold">
        {$t('wallet.wizard.enterPassword') === 'wallet.wizard.enterPassword'
          ? 'Enter wallet password'
          : $t('wallet.wizard.enterPassword')}
      </h3>
      <p class="text-sm text-muted-foreground">
        {$t('wallet.errors.passwordRequired') === 'wallet.errors.passwordRequired'
          ? 'Password required'
          : $t('wallet.errors.passwordRequired')}
      </p>
      <Input
        type="password"
        bind:value={keystorePasswordInput}
        placeholder={$t('placeholders.unlockPassword')}
      />
      {#if keystorePasswordError}
        <p class="text-sm text-red-500">{keystorePasswordError}</p>
      {/if}
      <div class="flex gap-2 justify-end">
        <Button variant="outline" on:click={() => { showKeystorePasswordPrompt = false; pendingKeystoreAddress = null; keystorePasswordInput = ''; }}>Cancel</Button>
        <Button on:click={confirmKeystorePassword} disabled={isUnlockingAccount}>
          {isUnlockingAccount
            ? ($t('actions.unlocking') === 'actions.unlocking' ? 'Unlocking...' : $t('actions.unlocking'))
            : ($t('wallet.wizard.unlockWallet') === 'wallet.wizard.unlockWallet' ? 'Unlock' : $t('wallet.wizard.unlockWallet'))}
        </Button>
      </div>
    </Card>
  </div>
{/if}

{#if showDeleteConfirm}
  <div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4">
    <Card class="w-full max-w-sm p-6 space-y-4">
      <h3 class="text-lg font-semibold">
        {$t('wallet.wizard.deleteWallet') === 'wallet.wizard.deleteWallet'
          ? 'Delete wallet'
          : $t('wallet.wizard.deleteWallet')}
      </h3>
      {#if pendingDeleteAddress}
        <p class="text-sm text-muted-foreground">
          {msg('wallet.wizard.deleteConfirm', `Delete wallet "${getWalletDisplayName(pendingDeleteAddress)}"? This cannot be undone.`)}
        </p>
      {/if}
      <div class="flex gap-2 justify-end">
        <Button variant="outline" on:click={() => { showDeleteConfirm = false; pendingDeleteAddress = null; }}>Cancel</Button>
        <Button variant="destructive" on:click={deleteWallet}>Delete</Button>
      </div>
    </Card>
  </div>
{/if}

<style>
</style>
