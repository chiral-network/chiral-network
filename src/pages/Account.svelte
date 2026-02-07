<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Wallet, Copy, ArrowUpRight, ArrowDownLeft, History, Coins, Plus, Import, BadgeX, KeyRound, FileText, AlertCircle, RefreshCw, Download } from 'lucide-svelte'
  import DropDown from "$lib/components/ui/dropDown.svelte";
  import { wallet, etcAccount, blacklist } from '$lib/stores'
  import { gethStatus } from '$lib/services/gethService'
  import { walletService, type WalletExportSnapshot } from '$lib/wallet';
  import { lockAccount } from '$lib/services/accountLock';
  import { transactions, transactionPagination, miningPagination } from '$lib/stores';
  import { derived } from 'svelte/store'
  import { invoke } from '@tauri-apps/api/core'
  import QRCode from 'qrcode'
  import { Html5QrcodeScanner as Html5QrcodeScannerClass } from 'html5-qrcode'
  import { tick } from 'svelte'
  import { onMount, getContext } from 'svelte'
  import { fade, fly } from 'svelte/transition'
  import { t, locale } from 'svelte-i18n'
  import { showToast } from '$lib/toast'
  import { get } from 'svelte/store'
  import { totalSpent, totalReceived, miningState, accurateTotals, isCalculatingAccurateTotals, accurateTotalsProgress, type Transaction } from '$lib/stores';
  import { goto } from '@mateothegreat/svelte5-router';

  const tr = (k: string, params?: Record<string, any>): string => $t(k, params)
  const msg = (k: string, fallback: string): string => {
    const val = $t(k);
    return val === k ? fallback : val;
  }
  const navigation = getContext('navigation') as { setCurrentPage: (page: string) => void };

  // SECURITY NOTE: Removed weak XOR obfuscation. Sensitive data should not be stored in frontend.
  // Use proper secure storage mechanisms in the backend instead.

  // HD wallet imports
  import MnemonicWizard from '$lib/components/wallet/MnemonicWizard.svelte'
  import AccountList from '$lib/components/wallet/AccountList.svelte'
  // HD helpers are used within MnemonicWizard/AccountList components

  // Transaction components
  import TransactionReceipt from '$lib/components/TransactionReceipt.svelte'

  // Validation utilities
  import { validatePrivateKeyFormat, RateLimiter } from '$lib/utils/validation'

  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

  // Interfaces - Transaction is now defined in stores.ts

  interface BlacklistEntry {
    chiral_address: string;
    reason: string;
    timestamp: Date;
    notes?: string;  // Make notes optional since it may not exist
  }
  
  let recipientAddress = ''
  let sendAmount = 0
  let rawAmountInput = '' // Track raw user input for validation
  let privateKeyVisible = false
  let showPending = false
  let importPrivateKey = ''
  type ImportedWalletSnapshot = WalletExportSnapshot & { transactions?: Transaction[] };
  let importedSnapshot: ImportedWalletSnapshot | null = null
  let isCreatingAccount = false
  let isImportingAccount = false
  let isGethRunning: boolean;
  let showQrCodeModal = false;
  let qrCodeDataUrl = ''
  let showScannerModal = false;
  let keystorePassword = '';
  let isSavingToKeystore = false;
  let keystoreSaveMessage = '';
  let keystoreAccounts: string[] = [];
  let selectedKeystoreAccount = '';
  let loadKeystorePassword = '';
  let isLoadingFromKeystore = false;
  let keystoreLoadMessage = '';
  let rememberKeystorePassword = false;

  // Rate limiter for keystore unlock (5 attempts per minute)
  const keystoreRateLimiter = new RateLimiter(5, 60000);
  let passwordStrength = '';
  let isPasswordValid = false;
  let passwordFeedback = '';
  
  // HD wallet state (frontend only)
  let showMnemonicWizard = false;
  let mnemonicMode: 'create' | 'import' = 'create';
  let hdMnemonic: string = '';
  let hdPassphrase: string = '';
  type HDAccountItem = { index: number; change: number; address: string; label?: string; privateKeyHex?: string };
  let hdAccounts: HDAccountItem[] = [];
  let chainId = 98765; // Default, will be fetched from backend

  // Transaction receipt modal state
  let selectedTransaction: Transaction | null = null;
  let showTransactionReceipt = false;
  
  // 2FA State
  // In a real app, this status should be loaded with the user's account data.
  let is2faEnabled = false; 
  let show2faSetupModal = false;
  let show2faPromptModal = false;
  let totpSetupInfo: { secret: string; qrCodeDataUrl: string } | null = null;
  let totpVerificationCode = '';
  let isVerifying2fa = false;
  let actionToConfirm: (() => any) | null = null;
  let totpActionCode = '';
  let isVerifyingAction = false;
  let twoFaErrorMessage = '';

  let twoFaPassword = ''; // To hold password for 2FA operations

  let Html5QrcodeScanner: InstanceType<typeof Html5QrcodeScannerClass> | null = null;

  // Enhanced validation states
  let validationWarning = '';
  let isAmountValid = true;
  let addressWarning = '';
  let isAddressValid = false;

  // Blacklist validation state
  let blacklistAddressWarning = '';
  let isBlacklistAddressValid = false;


   
  // Export feedback message
  let exportMessage = '';
  
  // Filtering state
  let filterType: 'transactions' | 'sent' | 'received' | 'mining' = 'transactions';
  let filterDateFrom: string = '';
  let filterDateTo: string = '';
  let sortDescending: boolean = true;
  let searchQuery: string = '';
  let txHashSearch: string = '';
  let minAmount: number = 0;
  let maxAmount: number | null = null;
  let blockNumberSearch: string = '';
  let minGasPrice: number = 0;
  let maxGasPrice: number | null = null;
  type TxHashLookupResult = {
    hash?: string;
    from?: string;
    to?: string | null;
    value?: string;
    block_number?: number | null;
    blockNumber?: number | null;
  };
  let hashSearchResult: TxHashLookupResult | null = null;
  let isSearchingHash = false;
  let hashSearchError = '';


  // Confirmation for sending transaction
  let isConfirming = false
  let countdown = 0
  let intervalId: number | null = null

  // Gas options state
  type GasOption = 'slow' | 'standard' | 'fast';
  interface GasPriceInfo {
    gwei: number;
    fee: number;
    time: string;
  }
  interface GasEstimate {
    gasLimit: number;
    gasPrices: {
      slow: GasPriceInfo;
      standard: GasPriceInfo;
      fast: GasPriceInfo;
    };
    networkCongestion: string;
  }
  let selectedGasOption: GasOption = 'standard';
  let gasEstimate: GasEstimate | null = null;
  let isLoadingGas = false;
  let gasError = '';

  // Derived display value
  $: estimatedFeeDisplay = gasEstimate 
    ? `${gasEstimate.gasPrices[selectedGasOption]?.fee.toFixed(6) ?? 0} CHR` 
    : '--';

  // Derive Geth running status from store
  $: isGethRunning = $gethStatus === 'running';

  // Start progressive loading when Geth becomes running or account changes
  // Only start if pagination has been initialized (oldestBlockScanned is not null)
  $: if (
    $etcAccount &&
    isGethRunning &&
    $transactionPagination.hasMore &&
    !$transactionPagination.isLoading &&
    $transactionPagination.oldestBlockScanned !== null &&
    $transactionPagination.accountAddress === $etcAccount.address
  ) {
    // Account address is part of the reactive dependency, so this triggers on account change
    walletService.startProgressiveLoading();
  }

  // Fetch balance when account changes
  $: if ($etcAccount && isGethRunning) {
    fetchBalance()
  }
  // Filter transactions to show only those related to current account
  $: if ($etcAccount) {
    const accountTransactions = $transactions.filter(tx =>
      // Mining rewards
      tx.from === 'Mining reward' ||
      tx.description?.toLowerCase().includes('block reward') ||
      // Transactions to/from this account
      tx.to?.toLowerCase() === $etcAccount.address.toLowerCase() ||
      tx.from?.toLowerCase() === $etcAccount.address.toLowerCase()
    );
    if (accountTransactions.length !== $transactions.length) {
      transactions.set(accountTransactions);
    }
}

  // Derived filtered transactions with safety checks
  $: filteredTransactions = (() => {
    try {
      if (!$transactions || !Array.isArray($transactions)) {
        return [];
      }

      return $transactions
        .filter(tx => {
          if (!tx) return false;

          // Default view now shows all types (sent/received/mining)
          const matchesType = filterType === 'transactions'
            ? (tx.type === 'sent' || tx.type === 'received' || tx.type === 'mining')
            : tx.type === filterType;

          let txDate: Date;
          try {
            txDate = tx.date instanceof Date ? tx.date : new Date(tx.date);
          } catch {
            return false; // Skip invalid dates
          }

          const fromOk = !filterDateFrom || txDate >= new Date(filterDateFrom + 'T00:00:00');
          const toOk = !filterDateTo || txDate <= new Date(filterDateTo + 'T23:59:59');

          // Search filter with null checks
          const matchesSearch = !searchQuery ||
            tx.description?.toLowerCase().includes(searchQuery.toLowerCase()) ||
            tx.to?.toLowerCase().includes(searchQuery.toLowerCase()) ||
            tx.from?.toLowerCase().includes(searchQuery.toLowerCase()) ||
            (tx.id && tx.id.toString().includes(searchQuery));

          // Amount range filter
          const amount = typeof tx.amount === 'number' ? tx.amount : parseFloat(String(tx.amount)) || 0;
          const matchesMinAmount = minAmount === 0 || amount >= minAmount;
          const matchesMaxAmount = maxAmount === null || amount <= maxAmount;

          // Gas price range filter (if gas data exists)
          const gasPrice = tx.gas_price ? parseFloat(String(tx.gas_price)) : 0;
          const matchesMinGas = minGasPrice === 0 || gasPrice >= minGasPrice;
          const matchesMaxGas = maxGasPrice === null || gasPrice <= maxGasPrice;

          // Block number filter
          const matchesBlock = !blockNumberSearch ||
            (tx.block_number && tx.block_number.toString() === blockNumberSearch);

          return matchesType && fromOk && toOk && matchesSearch &&
                 matchesMinAmount && matchesMaxAmount &&
                 matchesMinGas && matchesMaxGas && matchesBlock;
        })
        .slice()
        .sort((a, b) => {
          try {
            const dateA = a.date instanceof Date ? a.date : new Date(a.date);
            const dateB = b.date instanceof Date ? b.date : new Date(b.date);
            return sortDescending ? dateB.getTime() - dateA.getTime() : dateA.getTime() - dateB.getTime();
          } catch {
            return 0; // Keep original order if date comparison fails
          }
        });
    } catch (error) {
      console.error('Error filtering transactions:', error);
      return [];
    }
  })();

  // Address validation
  $: {
    if (!recipientAddress) {
      addressWarning = '';
      isAddressValid = false;
    } else if (!recipientAddress.startsWith('0x')) {
      addressWarning = tr('errors.address.mustStartWith0x');
      isAddressValid = false;
    } else if (recipientAddress.length !== 42) {
      addressWarning = tr('errors.address.mustBe42');
      isAddressValid = false;
    } else if (!isValidAddress(recipientAddress)) {
      addressWarning = tr('errors.address.mustBeHex');
      isAddressValid = false;
    } else if (isAddressBlacklisted(recipientAddress)) {
      addressWarning = tr('errors.address.blacklisted');
      isAddressValid = false;
    } else {
      addressWarning = '';
      isAddressValid = true;
    }
  }

  // Amount validation (accounts for gas fees)
  $: {
    if (rawAmountInput === '') {
      validationWarning = '';
      isAmountValid = false;
      sendAmount = 0;
    } else {
      const inputValue = parseFloat(rawAmountInput);
      const currentGasFee = gasEstimate?.gasPrices[selectedGasOption]?.fee ?? 0;
      const totalCost = inputValue + currentGasFee;

      if (isNaN(inputValue) || inputValue <= 0) {
        validationWarning = tr('errors.amount.invalid');
        isAmountValid = false;
        sendAmount = 0;
      } else if (inputValue < 0.01) {
        validationWarning = tr('errors.amount.min', { min: '0.01' });
        isAmountValid = false;
        sendAmount = 0;
      } else if (totalCost > $wallet.balance) {
        const shortage = (totalCost - $wallet.balance).toFixed(4);
        validationWarning = tr('errors.amount.insufficientWithGas', { values: { more: shortage } });
        isAmountValid = false;
        sendAmount = 0;
      } else {
        // Valid amount
        validationWarning = '';
        isAmountValid = true;
        sendAmount = inputValue;
      }
    }
  }

  // Add password validation logic
  $: {
    if (!keystorePassword) {
      passwordStrength = '';
      passwordFeedback = '';
      isPasswordValid = false;
    } else {
      // Check password requirements
      const hasMinLength = keystorePassword.length >= 8;
      const hasUppercase = /[A-Z]/.test(keystorePassword);
      const hasLowercase = /[a-z]/.test(keystorePassword);
      const hasNumber = /[0-9]/.test(keystorePassword);
      const hasSpecial = /[!@#$%^&*(),.?":{}|<>]/.test(keystorePassword);

      // Calculate strength
      let strength = 0;
      if (hasMinLength) strength++;
      if (hasUppercase) strength++;
      if (hasLowercase) strength++;
      if (hasNumber) strength++; 
      if (hasSpecial) strength++;

      // Set feedback based on strength
      if (strength < 2) {
        passwordStrength = 'weak';
        passwordFeedback = tr('password.weak');
        isPasswordValid = false;
      } else if (strength < 4) {
        passwordStrength = 'medium';
        passwordFeedback = tr('password.medium');
        isPasswordValid = false;
      } else {
        passwordStrength = 'strong';
        passwordFeedback = tr('password.strong');
        isPasswordValid = true;
      }
    }
  }


  // Blacklist address validation (same as Send Coins validation)
  $: {
    const addr = newBlacklistEntry.chiral_address;

    if (!addr) {
      blacklistAddressWarning = '';
      isBlacklistAddressValid = false;
    } else if (!addr.startsWith('0x')) {
      blacklistAddressWarning = tr('errors.address.mustStartWith0x');
      isBlacklistAddressValid = false;
    } else if (addr.length !== 42) {
      blacklistAddressWarning = tr('errors.address.mustBe42');
      isBlacklistAddressValid = false;
    } else if (!isValidAddress(addr)) {
      blacklistAddressWarning = tr('errors.address.mustBeHex');
      isBlacklistAddressValid = false;
    } else if (isAddressAlreadyBlacklisted(addr)) {
      blacklistAddressWarning = tr('blacklist.errors.alreadyExists');
      isBlacklistAddressValid = false;
    } else if (isOwnAddress(addr)) {
      blacklistAddressWarning = tr('blacklist.errors.ownAddress');
      isBlacklistAddressValid = false;
    } else {
      blacklistAddressWarning = '';
      isBlacklistAddressValid = true;
    }
  }
  
  // Prepare options for the DropDown component
  $: keystoreOptions = keystoreAccounts.map(acc => ({ value: acc, label: acc }));

  // When logged out, if a keystore account is selected, try to load its saved password.
  $: if (!$etcAccount && selectedKeystoreAccount) {
    loadSavedPassword(selectedKeystoreAccount);
  }

  // Enhanced address validation function
  function isValidAddress(address: string): boolean {
    // Check that everything after 0x is hexadecimal
    const hexPart = address.slice(2);
    if (hexPart.length === 0) return false;
    
    const hexRegex = /^[a-fA-F0-9]+$/;
    return hexRegex.test(hexPart);
  }

  // Add helper function to check blacklist
  function isAddressBlacklisted(address: string): boolean {
    return $blacklist.some(entry => 
      entry.chiral_address.toLowerCase() === address.toLowerCase()
    );
  }

  function copyAddress() {
    const addressToCopy = $etcAccount ? $etcAccount.address : $wallet.address;
    navigator.clipboard.writeText(addressToCopy);
    showToast(tr('toasts.account.addressCopied'), 'success')
  }

  function copyPrivateKey() {
    with2FA(async () => {
      let privateKeyToCopy = $etcAccount ? $etcAccount.private_key : '';
      
      // If private key is not in frontend store, fetch it from backend
      if (!privateKeyToCopy && isTauri) {
        try {
          privateKeyToCopy = await invoke<string>('get_active_account_private_key');
        } catch (error) {
          console.error('Failed to get private key from backend:', error);
          showToast(tr('toasts.account.privateKey.fetchError'), 'error');
          return;
        }
      }
      
      if (privateKeyToCopy) {
        navigator.clipboard.writeText(privateKeyToCopy);
        showToast(tr('toasts.account.privateKey.copied'), 'success');
      }
      else {
        showToast(tr('toasts.account.privateKey.missing'), 'error');
      }
    });
  }
    
  function exportWallet() {
    with2FA(async () => {
      try {
        const snapshot = await walletService.exportSnapshot({ includePrivateKey: true });
        const dataStr = JSON.stringify(snapshot, null, 2);
        const dataBlob = new Blob([dataStr], { type: 'application/json' });
        const fileName = `chiral-wallet-export-${new Date().toISOString().split('T')[0]}.json`;

        if (isTauri) {
          try {
            const storagePath = await invoke<string>('get_download_directory');
            await invoke('ensure_directory_exists', { path: storagePath });
            const { join } = await import('@tauri-apps/api/path');
            const exportPath = await join(storagePath, fileName);
            const { writeFile } = await import('@tauri-apps/plugin-fs');
            await writeFile(exportPath, new TextEncoder().encode(dataStr));
            exportMessage = tr('wallet.exportSuccess');
            setTimeout(() => exportMessage = '', 3000);
            return;
          } catch (error) {
            console.error('Tauri export failed, falling back to browser flow:', error);
          }
        }
        
        // Check if the File System Access API is supported
        if ('showSaveFilePicker' in window) {
          try {
            const fileHandle = await (window as any).showSaveFilePicker({
              suggestedName: fileName,
              types: [{
                description: 'JSON files',
                accept: {
                  'application/json': ['.json'],
                },
              }],
            });
            
            const writable = await fileHandle.createWritable();
            await writable.write(dataBlob);
            await writable.close();

            exportMessage = tr('wallet.exportSuccess');
          } catch (error: unknown) {
            if (error instanceof Error && error.name === 'AbortError') {
              // User cancelled, don't show error message
              return;
            }
            throw error;
          }
        } else {
          // Fallback for browsers that don't support File System Access API
          const url = URL.createObjectURL(dataBlob);
          const link = document.createElement('a');
          link.href = url;
          link.download = fileName;
          document.body.appendChild(link);
          link.click();
          document.body.removeChild(link);
          URL.revokeObjectURL(url);

          exportMessage = tr('wallet.exportSuccess');
        }
        
        setTimeout(() => exportMessage = '', 3000);
      } catch (error) {
        console.error('Export failed:', error);
        exportMessage = tr('errors.exportFailed');
        setTimeout(() => exportMessage = '', 3000);
      }
    });
  }
  
  function handleSendClick() {
    if (!isAddressValid || !isAmountValid || sendAmount <= 0) return

    if (isConfirming) {
      // Cancel if user taps again during countdown
      cancelCountdown()
      return
    }

    with2FA(startCountdown);
  }

  function startCountdown() {
    isConfirming = true
    countdown = 5

    intervalId = window.setInterval(() => {
      countdown--
      if (countdown <= 0) {
        clearInterval(intervalId!)
        intervalId = null
        isConfirming = false
        sendTransaction() 
      }
    }, 1000)
  }

  function cancelCountdown() {
    if (intervalId) {
      clearInterval(intervalId)
      intervalId = null
    }
    isConfirming = false
    countdown = 0
    // User intentionally cancelled during countdown
    showToast(tr('toasts.account.transaction.cancelled'), 'warning')
  }

  async function sendTransaction() {
    if (!isAddressValid || !isAmountValid || sendAmount <= 0) return
    
    try {
      await walletService.sendTransaction(recipientAddress, sendAmount)
      
      // Clear form
      recipientAddress = ''
      sendAmount = 0
      rawAmountInput = ''

      showToast(tr('toasts.account.transaction.submitted'), 'success')
      
      // Refresh balance after a delay to allow transaction to be mined
      // Poll every 2 seconds for 30 seconds to catch the confirmation
      let pollCount = 0;
      const pollInterval = setInterval(async () => {
        pollCount++;
        await fetchBalance();
        if (pollCount >= 15) {
          clearInterval(pollInterval);
        }
      }, 2000);
      
    } catch (error) {
      console.error('Transaction failed:', error)
      showToast(
        tr('toasts.account.transaction.error', { values: { error: String(error) } }),
        'error'
      )
      
      // Refresh balance to get accurate state
      await fetchBalance()
    }
  }

  function formatDate(date: Date): string {
    const loc = get(locale) || 'en-US'
    return new Intl.DateTimeFormat(typeof loc === 'string' ? loc : 'en-US', { month: 'short', day: 'numeric', year: 'numeric' }).format(date)
  }

  function handleTransactionClick(tx: Transaction) {
    selectedTransaction = tx;
    showTransactionReceipt = true;
  }

  function closeTransactionReceipt() {
    showTransactionReceipt = false;
    selectedTransaction = null;
  }

  async function searchByTransactionHash() {
    if (!txHashSearch || !isTauri) {
      hashSearchResult = null;
      hashSearchError = '';
      return;
    }

    // Validate hash format
    if (!txHashSearch.startsWith('0x') || txHashSearch.length !== 66) {
      hashSearchError = 'Invalid transaction hash format (must be 0x followed by 64 hex characters)';
      hashSearchResult = null;
      return;
    }

    isSearchingHash = true;
    hashSearchError = '';
    hashSearchResult = null;

    try {
      const result = await invoke<TxHashLookupResult | null>('get_transaction_by_hash', { txHash: txHashSearch });
      if (result) {
        hashSearchResult = result;
      } else {
        hashSearchError = 'Transaction not found';
      }
    } catch (error) {
      console.error('Failed to search transaction by hash:', error);
      hashSearchError = 'Failed to retrieve transaction';
    } finally {
      isSearchingHash = false;
    }
  }

  // Ensure wallet.pendingTransactions matches actual pending transactions
  const pendingCount = derived(transactions, $txs => $txs.filter(tx => tx.status === 'pending').length);

  // Ensure pendingCount is used (for linter)
  $: void $pendingCount;
    
  onMount(() => {
    // Initialize wallet service asynchronously
    walletService.initialize().then(async () => {
      await loadKeystoreAccountsList();

      // Fetch chain ID from backend
      if (isTauri) {
        try {
          chainId = await invoke<number>('get_chain_id');
        } catch (error) {
          console.warn('Failed to fetch chain ID from backend, using default:', error);
        }
      }

      if ($etcAccount && isGethRunning) {
        // IMPORTANT: refreshTransactions must run BEFORE refreshBalance
        await walletService.refreshTransactions();
        await walletService.refreshBalance();

        // Start progressive loading of all transactions in background
        walletService.startProgressiveLoading();
      }
    });

    // Cleanup on unmount
    return () => {
      walletService.stopProgressiveLoading();
    };
  })

  async function fetchBalance() {
    console.log('[Account.fetchBalance] Called: isTauri=', isTauri, 'isGethRunning=', isGethRunning, 'etcAccount=', $etcAccount?.address);
    if (!isTauri || !isGethRunning || !$etcAccount) {
      console.log('[Account.fetchBalance] Skipping: isTauri=', isTauri, 'isGethRunning=', isGethRunning, 'hasAccount=', !!$etcAccount);
      return;
    }
    try {
      await walletService.refreshBalance()
      console.log('[Account.fetchBalance] After refresh, wallet balance:', $wallet.balance);
    } catch (error) {
      console.error('[Account.fetchBalance] Failed to fetch balance:', error)
    }
  }

  async function calculateAccurateTotals() {
    try {
      accurateTotalsError = null;
      await walletService.calculateAccurateTotals();
      console.debug('Accurate totals calculated successfully');
    } catch (error) {
      console.error('Failed to calculate accurate totals:', error);
      accurateTotalsError = String(error);
    }
  }

  // Automatically calculate accurate totals when account is loaded
  let didAutoCalculateAccurateTotals = false;
  let lastAccurateTotalsAddress: string | null = null;
  let accurateTotalsError: string | null = null;

  // Reset auto-trigger when account changes
  $: if ($etcAccount?.address && $etcAccount.address !== lastAccurateTotalsAddress) {
    lastAccurateTotalsAddress = $etcAccount.address;
    didAutoCalculateAccurateTotals = false;
    accurateTotalsError = null;
  }

  $: if (
    $etcAccount &&
    isGethRunning &&
    !$accurateTotals &&
    !$isCalculatingAccurateTotals &&
    !didAutoCalculateAccurateTotals
  ) {
    didAutoCalculateAccurateTotals = true;
    calculateAccurateTotals();
  }


  async function createChiralAccount() {
  isCreatingAccount = true
  try {
    const account = await walletService.createAccount()

    wallet.update(w => ({
      ...w,
      address: account.address,
      balance: 0,
      pendingTransactions: 0
    }))

    transactions.set([])
    blacklist.set([])   

    showToast(tr('toasts.account.created'), 'success')
    
    if (isGethRunning) {
      await walletService.refreshBalance()
    }
  } catch (error) {
    console.error('Failed to create Chiral account:', error)
    showToast(
      tr('toasts.account.createError', { values: { error: String(error) } }),
      'error'
    )
    alert(tr('errors.createAccount', { error: String(error) }))
  } finally {
    isCreatingAccount = false
  }
}

  async function saveToKeystore() {
    if (!keystorePassword || !$etcAccount) return;

    isSavingToKeystore = true;
    keystoreSaveMessage = '';

    try {
        if (isTauri) {
            // Explicitly pass the account from the frontend store
            await walletService.saveToKeystore(keystorePassword, $etcAccount);
            keystoreSaveMessage = tr('keystore.success');
        } else {
            await new Promise(resolve => setTimeout(resolve, 1000));
            keystoreSaveMessage = tr('keystore.successSimulated');
        }
        keystorePassword = ''; // Clear password after saving
    } catch (error) {
        console.error('Failed to save to keystore:', error);
        keystoreSaveMessage = tr('keystore.error', { error: String(error) });
    } finally {
        isSavingToKeystore = false;
        setTimeout(() => keystoreSaveMessage = '', 4000);
    }
  }

  async function scanQrCode() {
    // 1. Show the modal
    showScannerModal = true;

    // 2. Wait for Svelte to render the modal in the DOM
    await tick();

    // 3. This function runs when a QR code is successfully scanned
    function onScanSuccess(decodedText: string, _decodedResult: unknown) {
      // Handle the scanned code
      // Paste the address into the input field
      recipientAddress = decodedText;
      
      // Stop the scanner and close the modal
      if (Html5QrcodeScanner) {
        Html5QrcodeScanner.clear();
        Html5QrcodeScanner = null;
      }
      showScannerModal = false;
    }

    // 4. This function can handle errors (optional)
    function onScanFailure() {
      // handle scan failure, usually better to ignore and let the user keep trying
      // console.warn(`Code scan error`);
    }

    // 5. Create and render the scanner
    Html5QrcodeScanner = new Html5QrcodeScannerClass(
      "qr-reader", // The ID of the div we created in the HTML
      { fps: 10, qrbox: { width: 250, height: 250 } },
      /* verbose= */ false);
    Html5QrcodeScanner.render(onScanSuccess, onScanFailure);
  }

  // --- We also need a way to stop the scanner if the user just clicks "Cancel" ---
  // We can use a reactive statement for this.
  $: if (!showScannerModal && Html5QrcodeScanner) {
    Html5QrcodeScanner.clear();
    Html5QrcodeScanner = null;
  }

  async function importChiralAccount() {
    if (!importPrivateKey) return

    // Validate private key format before attempting import
    const validation = validatePrivateKeyFormat(importPrivateKey)
    if (!validation.isValid) {
      showToast(validation.error || tr('toasts.account.import.invalidFormat'), 'error')
      return
    }

    isImportingAccount = true
    try {
      const account = await walletService.importAccount(importPrivateKey)
      if (importedSnapshot) {
        const snapshot = importedSnapshot;
        if (typeof snapshot.balance === 'number') {
          wallet.update(w => ({ ...w, balance: snapshot.balance, actualBalance: snapshot.balance }))
        }
        if (Array.isArray(snapshot.transactions)) {
          const hydrated = snapshot.transactions.map((tx: Transaction) => ({
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
      importPrivateKey = ''
      importedSnapshot = null


      showToast(tr('toasts.account.import.success'), 'success')

      // Match keystore load behavior: hydrate transactions and balance right away
      if (isGethRunning) {
        await walletService.refreshTransactions();
        await walletService.refreshBalance();
        walletService.startProgressiveLoading();
      }
    } catch (error) {
      console.error('Failed to import Chiral account:', error)


      showToast(
        tr('toasts.account.import.error', { values: { error: String(error) } }),
        'error'
      )

      alert('Failed to import account: ' + error)
    } finally {
      isImportingAccount = false
    }
  }

  async function loadPrivateKeyFromFile() {
    try {
      const msg = (key: string, fallback: string) => {
        const val = $t(key);
        return val === key ? fallback : val;
      };

      // Create a file input element
      const fileInput = document.createElement('input');
      fileInput.type = 'file';
      fileInput.accept = '.json';
      fileInput.style.display = 'none';
      
      // Handle file selection
      fileInput.onchange = async (event) => {
        const file = (event.target as HTMLInputElement).files?.[0];
        if (!file) return;
        
        try {
          const fileContent = await file.text();
          const accountData = JSON.parse(fileContent);
          
          // Validate the JSON structure
          if (!accountData.privateKey && !accountData.private_key) {
            showToast(msg('toasts.account.import.fileInvalid', 'Invalid wallet file (missing private key)'), 'error');
            return;
          }
          
          // Extract and set the private key
          importPrivateKey = accountData.privateKey ?? accountData.private_key;
          importedSnapshot = accountData;

          // Hydrate balance/transactions immediately if present
          if (typeof accountData.balance === 'number') {
            wallet.update(w => ({ ...w, balance: accountData.balance, actualBalance: accountData.balance }));
          }
          if (Array.isArray(accountData.transactions)) {
            const hydrated = accountData.transactions.map((tx: Transaction) => ({
              ...tx,
              date: tx.date ? new Date(tx.date) : new Date()
            }));
            transactions.set(hydrated);
          }

          showToast(msg('toasts.account.import.fileSuccess', 'Wallet file loaded. Ready to import.'), 'success');
          
        } catch (error) {
          console.error('Error reading file:', error);
          showToast(
            msg('toasts.account.import.fileReadError', `Error reading wallet file: ${String(error) }`),
            'error'
          );
        }
      };
      
      // Trigger file selection
      document.body.appendChild(fileInput);
      fileInput.click();
      document.body.removeChild(fileInput);
      
    } catch (error) {
      console.error('Error loading file:', error);
      const msg = (key: string, fallback: string) => {
        const val = $t(key);
        return val === key ? fallback : val;
      };
      showToast(
        msg('toasts.account.import.fileLoadError', `Error loading wallet file: ${String(error) }`),
        'error'
      );
    }
  }

  // HD wallet handlers
  function openCreateMnemonic() {
    mnemonicMode = 'create';
    showMnemonicWizard = true;
  }
  function openImportMnemonic() {
    mnemonicMode = 'import';
    showMnemonicWizard = true;
  }
  function closeMnemonicWizard() {
    showMnemonicWizard = false;
  }
  async function completeMnemonicWizard(ev: { mnemonic: string, passphrase: string, account: { address: string, privateKeyHex: string, index: number, change: number }, name?: string }) {
    showMnemonicWizard = false;
    hdMnemonic = ev.mnemonic;
    hdPassphrase = ev.passphrase || '';
    // set first account
    hdAccounts = [{ index: ev.account.index, change: ev.account.change, address: ev.account.address, privateKeyHex: ev.account.privateKeyHex, label: ev.name || 'Account 0' }];
    
    // Import to backend to set as active account
    const privateKeyWithPrefix = '0x' + ev.account.privateKeyHex;
    if (isTauri) {
      try {
        await invoke('import_chiral_account', { privateKey: privateKeyWithPrefix });
      } catch (error) {
        console.error('Failed to set backend account:', error);
      }
    }
    
    // set as active (frontend)
    etcAccount.set({ address: ev.account.address, private_key: privateKeyWithPrefix });
    wallet.update(w => ({ ...w, address: ev.account.address }));
    if (isGethRunning) { await fetchBalance(); }
  }
  function onHDAccountsChange(updated: HDAccountItem[]) {
    hdAccounts = updated;
  }

  async function loadKeystoreAccountsList() {
    try {
      if (!isTauri) return;
      const accounts = await walletService.listKeystoreAccounts();
      keystoreAccounts = accounts;
      if (accounts.length > 0) {
        selectedKeystoreAccount = accounts[0];
      }
    } catch (error) {
      console.error('Failed to list keystore accounts:', error);
    }
  }

  function loadSavedPassword(address: string) {
    try {
      const savedPasswordsRaw = localStorage.getItem('chiral_keystore_passwords');
      if (savedPasswordsRaw) {
        const savedPasswords: Record<string, { pass: string, expires: number }> = JSON.parse(savedPasswordsRaw);
        const saved = savedPasswords[address];
        if (saved) {
          const now = new Date().getTime();
          if (now < saved.expires) {
            loadKeystorePassword = saved.pass;
            rememberKeystorePassword = true;
          } else {
            // Password expired, remove it
            saveOrClearPassword(address, ''); // This will clear it if checkbox is unchecked
          }
        }
        else {
          // Clear if no password is saved for this account
          loadKeystorePassword = '';
          rememberKeystorePassword = false;
        }
      }
    } catch (e) {
      console.error("Failed to load saved password from localStorage", e);
    }
  }

  async function loadFromKeystore() {
    if (!selectedKeystoreAccount || !loadKeystorePassword) return;

    // Rate limiting: prevent brute force attacks
    if (!keystoreRateLimiter.checkLimit('keystore-unlock')) {
      keystoreLoadMessage = 'Too many unlock attempts. Please wait 1 minute before trying again.';
      setTimeout(() => keystoreLoadMessage = '', 4000);
      return;
    }

    isLoadingFromKeystore = true;
    keystoreLoadMessage = '';

    try {
        if (isTauri) {
            const account = await walletService.loadFromKeystore(selectedKeystoreAccount, loadKeystorePassword);

            if (account.address.toLowerCase() !== selectedKeystoreAccount.toLowerCase()) {
                throw new Error(tr('keystore.load.addressMismatch'));
            }

            // Success - reset rate limiter for this account
            keystoreRateLimiter.reset('keystore-unlock');

            saveOrClearPassword(selectedKeystoreAccount, loadKeystorePassword);

            // The wallet service already sets etcAccount and clears transactions;
            // ensure the UI store mirrors the loaded address.
            wallet.update(w => ({
                ...w,
                address: account.address
            }));

            // Clear sensitive data
            loadKeystorePassword = '';

            // After loading the keystore, fetch the full state so balances and history populate.
            if (isGethRunning) {
                await walletService.refreshTransactions();
                await walletService.refreshBalance();
                walletService.startProgressiveLoading();
            }

            keystoreLoadMessage = tr('keystore.load.success');

        } else {
            // Web demo mode simulation
            // Save or clear the password from local storage based on the checkbox
            saveOrClearPassword(selectedKeystoreAccount, loadKeystorePassword);
            await new Promise(resolve => setTimeout(resolve, 1000));
            keystoreRateLimiter.reset('keystore-unlock'); // Reset on success in demo mode too
            keystoreLoadMessage = tr('keystore.load.successSimulated');
        }

    } catch (error) {
        console.error('Failed to load from keystore:', error);
        keystoreLoadMessage = tr('keystore.load.error', { error: String(error) });

        // Clear sensitive data on error
        // Note: Rate limiter is NOT reset on failure - failed attempts count toward limit
        loadKeystorePassword = '';
    } finally {
        isLoadingFromKeystore = false;
        setTimeout(() => keystoreLoadMessage = '', 4000);
    }
  }

  async function deleteKeystoreAccount() {
    if (!selectedKeystoreAccount) return;

    const confirmMsg = tr('keystore.delete.confirm', { values: { address: selectedKeystoreAccount } });
    if (!confirm(confirmMsg)) return;

    try {
      await walletService.deleteKeystoreAccount(selectedKeystoreAccount);
      // Refresh list
      await loadKeystoreAccountsList();
      // Clear selection and password
      selectedKeystoreAccount = '';
      loadKeystorePassword = '';
      showToast(tr('keystore.delete.success'), 'success');
    } catch (error) {
      console.error('Failed to delete keystore account:', error);
      showToast(tr('keystore.delete.error', { values: { error: String(error) } }), 'error');
    }
  }

  function saveOrClearPassword(address: string, password: string) {
    try {
      const savedPasswordsRaw = localStorage.getItem('chiral_keystore_passwords');
      let savedPasswords = savedPasswordsRaw ? JSON.parse(savedPasswordsRaw) : {};
  
      if (rememberKeystorePassword) {
        const expires = new Date().getTime() + 30 * 24 * 60 * 60 * 1000; // 30 days from now
        savedPasswords[address] = { pass: password, expires };
      } else {
        delete savedPasswords[address];
      }

      localStorage.setItem('chiral_keystore_passwords', JSON.stringify(savedPasswords));
    } catch (e) {
      console.error("Failed to save password to localStorage", e);
    }
  }

  // Reactive statement to check 2FA status when user logs in
  $: if ($etcAccount && isTauri) {
    check2faStatus();
  }

  async function check2faStatus() {
    try {
      is2faEnabled = await walletService.isTwoFactorEnabled();
    } catch (error) {
      console.error('Failed to check 2FA status:', error);
      // is2faEnabled will remain false, which is a safe default.
    }
  }

  // --- 2FA Functions ---

  // This would be called by the "Enable 2FA" button
  async function setup2FA() {
    if (!isTauri) {
      showToast(tr('toasts.account.2fa.desktopOnly'), 'warning');
      return;
    }

    try {
      const setup = await walletService.generateTwoFactorSetup();
      const qrCodeDataUrl = await QRCode.toDataURL(setup.otpauthUrl);

      totpSetupInfo = { secret: setup.secret, qrCodeDataUrl };
      show2faSetupModal = true;
      totpVerificationCode = '';
      twoFaErrorMessage = '';
    } catch (err) {
      console.error('Failed to setup 2FA:', err);
      showToast(
        tr('toasts.account.2fa.setupError', { values: { error: String(err) } }),
        'error'
      );
    }
  }

  // Called from the setup modal to verify and enable 2FA
  async function verifyAndEnable2FA() {
    if (!totpSetupInfo || !totpVerificationCode) return;
    isVerifying2fa = true;
    twoFaErrorMessage = '';

    try {
      const success = await walletService.verifyAndEnableTwoFactor(
        totpSetupInfo.secret,
        totpVerificationCode,
        twoFaPassword
      );

      if (success) {
        is2faEnabled = true; 
        show2faSetupModal = false;
        showToast(tr('toasts.account.2fa.enabled'), 'success');
      } else {
        // Don't clear password, but clear code
        twoFaErrorMessage = 'Invalid code. Please try again.';
        totpVerificationCode = '';
      }
    } catch (error) {
      twoFaErrorMessage = String(error);
    } finally {
      isVerifying2fa = false;
    }
  }

  // This is the main wrapper for protected actions
  function with2FA(action: () => any) {
    if (!is2faEnabled) {
      action();
      return;
    }
    
    // If 2FA is enabled, show the prompt
    actionToConfirm = action;
    totpActionCode = '';
    twoFaErrorMessage = '';
    show2faPromptModal = true;
  }

  // Called from the 2FA prompt modal
  async function confirmActionWith2FA() {
    if (!actionToConfirm || !totpActionCode) return;
    isVerifyingAction = true;
    twoFaErrorMessage = '';

    try {
      const success = await walletService.verifyTwoFactor(totpActionCode, twoFaPassword);

      if (success) {
        show2faPromptModal = false;
        actionToConfirm(); // Execute the original action
      } else {
        twoFaErrorMessage = 'Invalid code. Please try again.';
        totpActionCode = ''; // Clear input on failure
      }
    } catch (error) {
      twoFaErrorMessage = String(error);
    } finally {
      isVerifyingAction = false;
      // Only clear the action if the modal was successfully closed
      if (!show2faPromptModal) {
        actionToConfirm = null;
      }
    }
  }

  // To disable 2FA (this action is also protected by 2FA)
  function disable2FA() {
    with2FA(async () => {
      try { // The password is provided in the with2FA prompt
        await walletService.disableTwoFactor(twoFaPassword);
        is2faEnabled = false;
        showToast(tr('toasts.account.2fa.disabled'), 'warning');
      } catch (error) {
        console.error('Failed to disable 2FA:', error);
        showToast(
          tr('toasts.account.2fa.disableError', { values: { error: String(error) } }),
          'error'
        );
      }
    });
  }

  function togglePrivateKeyVisibility() {
    if (privateKeyVisible) {
        // Hiding doesn't need 2FA
        privateKeyVisible = false;
    } else {
        // Showing needs 2FA
        with2FA(() => {
            privateKeyVisible = true;
        });
    }
  }

  let newBlacklistEntry = {
    chiral_address: "",
    reason: ""
  }

  
  //Guard add with validity check
  async function addBlacklistEntry() {
  if (!isBlacklistFormValid) return;
  
  const newEntry = { 
    chiral_address: newBlacklistEntry.chiral_address, 
    reason: newBlacklistEntry.reason, 
    timestamp: new Date() 
  };
  
  // Add to store
  blacklist.update(entries => [...entries, newEntry]);
  
  // Disconnect peer if currently connected
  try {
    await invoke('disconnect_peer', { 
      peerId: newEntry.chiral_address 
    });
    console.log(`Disconnected blacklisted peer: ${newEntry.chiral_address}`);
  } catch (error) {
    // Peer not connected or already disconnected - this is fine
    console.log('Peer not connected or already disconnected:', error);
  }
  
  // Clear form
  newBlacklistEntry.chiral_address = "";
  newBlacklistEntry.reason = "";
  
  // Show success message
  showToast($t('account.blacklist.added'), 'success');
}

  function removeBlacklistEntry(chiral_address: string) {
    if (confirm(tr('blacklist.confirm.remove', { address: chiral_address }))) {
      blacklist.update(entries => 
        entries.filter(entry => entry.chiral_address !== chiral_address)
      );
    }
  }

  // Additional variables for enhanced blacklist functionality
  let blacklistSearch = '';
  let importFileInput: HTMLInputElement;
  let editingEntry: number | null = null;
  let editReason = '';

  function startEditEntry(index: number) {
    editingEntry = index;
    editReason = $blacklist[index].reason;
  }

  function cancelEdit() {
    editingEntry = null;
    editReason = '';
  }

  function saveEdit() {
    if (editingEntry !== null && editReason.trim() !== '') {
      blacklist.update(entries => {
        const updated = [...entries];
        updated[editingEntry!] = { ...updated[editingEntry!], reason: editReason.trim() };
        return updated;
      });
    }
    cancelEdit();
  }

  // Enhanced validation
  $: isBlacklistFormValid = 
    newBlacklistEntry.reason.trim() !== '' &&
    isBlacklistAddressValid;

  // Filtered blacklist for search
  $: filteredBlacklist = $blacklist.filter(entry => 
    entry.chiral_address.toLowerCase().includes(blacklistSearch.toLowerCase()) ||
    entry.reason.toLowerCase().includes(blacklistSearch.toLowerCase())
  );


  function isAddressAlreadyBlacklisted(address: string) {
    if (!address) return false;
    return $blacklist.some(entry => 
      entry.chiral_address.toLowerCase() === address.toLowerCase()
    );
  }

  function isOwnAddress(address: string) {
    if (!address || !$etcAccount) return false;
    return address.toLowerCase() === $etcAccount.address.toLowerCase();
  }

  function clearAllBlacklist() {
    const count = $blacklist.length;
    if (window.confirm(`Remove all ${count} blacklisted addresses?`)) {
        blacklist.set([]);
        blacklistSearch = '';
    }
  }

  function clearBlacklistSearch() {
    blacklistSearch = '';
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    // Could show a brief "Copied!" message
  }


  function exportBlacklist() {
    const data = {
      version: "1.0",
      exported: new Date().toISOString(),
      blacklist: $blacklist
    };

    const blob = new Blob([JSON.stringify(data, null, 2)], {
      type: 'application/json'
    });

    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `chiral-blacklist-${new Date().toISOString().split('T')[0]}.json`;
    link.click();
    URL.revokeObjectURL(url);
  }

  function exportTransactionsCSV() {
    try {
      console.log('Export CSV clicked, transaction count:', filteredTransactions.length);

      if (filteredTransactions.length === 0) {
        showToast('No transactions to export', 'warning');
        return;
      }

      // CSV header
      const headers = ['Date', 'Type', 'Amount (CN)', 'From', 'To', 'Description', 'Status', 'Hash', 'Block Number'];

      // Convert filtered transactions to CSV rows
      const rows = filteredTransactions.map(tx => {
        const date = tx.date instanceof Date ? tx.date.toISOString() : new Date(tx.date).toISOString();

        // Translate transaction type
        let translatedType: string = tx.type;
        if (tx.type === 'sent') {
          translatedType = tr('filters.typeSent');
        } else if (tx.type === 'received') {
          translatedType = tr('filters.typeReceived');
        } else if (tx.type === 'mining') {
          translatedType = tr('filters.typeMining');
        }

        const amount = tx.amount?.toFixed(8) || '0.00000000';
        const from = tx.from || '';
        const to = tx.to || '';
        const description = (tx.description || '').replace(/"/g, '""'); // Escape quotes
        const status = tx.status || '';
        const hash = tx.hash || tx.txHash || '';
        const blockNumber = tx.block_number || '';

        return [date, translatedType, amount, from, to, `"${description}"`, status, hash, blockNumber].join(',');
      });

      // Combine header and rows
      const csv = [headers.join(','), ...rows].join('\n');

      // Create and download file
      const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = `chiral-transactions-${new Date().toISOString().split('T')[0]}.csv`;
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      URL.revokeObjectURL(url);

      console.log('CSV export completed successfully');
      showToast(tr('transactions.exportSuccess', { count: filteredTransactions.length }), 'success');
    } catch (error) {
      console.error('CSV export error:', error);
      const errorMessage = error instanceof Error ? error.message : String(error);
      showToast('Failed to export transactions: ' + errorMessage, 'error');
    }
  }

  function handleImportFile(event: Event) {
    const target = event.target as HTMLInputElement;
    const file = target?.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (e: ProgressEvent<FileReader>) => {
      try {
        const result = e.target?.result;
        if (typeof result !== 'string') return;
        
        const data = JSON.parse(result);
        
        if (data.blacklist && Array.isArray(data.blacklist)) {
          const imported = data.blacklist.filter((entry: Partial<BlacklistEntry>) =>
            entry.chiral_address && 
            entry.reason &&
            isValidAddress(entry.chiral_address) &&
            !isAddressAlreadyBlacklisted(entry.chiral_address)
          ).map((entry: Partial<BlacklistEntry>) => ({
            chiral_address: entry.chiral_address!,
            reason: entry.reason!,
            timestamp: entry.timestamp ? new Date(entry.timestamp) : new Date()
          }));
          
          if (imported.length > 0) {
            // Force reactivity by creating new array reference
            blacklist.update(entries => [...entries, ...imported]);
            alert(tr('blacklist.import.success', { count: imported.length }));
          } else {
            alert(tr('blacklist.import.none'));
          }
        } else {
          alert(tr('blacklist.import.invalid'));
        }
      } catch (error) {
        alert(tr('blacklist.import.parseError'));
      }
    };
    
    reader.readAsText(file);
    target.value = ''; // Reset input
  }

  // Enhanced keyboard event handling
  function handleEditKeydown(e: CustomEvent<KeyboardEvent>) {
    if (e.detail.key === 'Enter') {
      e.detail.preventDefault();
      saveEdit();
    }
    if (e.detail.key === 'Escape') {
      e.detail.preventDefault();
      cancelEdit();
    }
  }

  // Helper function to set max amount
  async function setMaxAmount() {
    // If we don't have a gas estimate yet, fetch it first
    if (!gasEstimate && isGethRunning) {
      await fetchGasEstimate();
    }
    
    // Calculate max amount accounting for gas fee
    const currentGasFee = gasEstimate?.gasPrices?.[selectedGasOption]?.fee ?? 0;
    const maxAmount = Math.max(0, $wallet.balance - currentGasFee);
    
    // Round DOWN to 4 decimal places to ensure we don't exceed available balance
    // Using floor instead of round to be safe
    const roundedMax = Math.floor(maxAmount * 10000) / 10000;
    
    rawAmountInput = roundedMax.toFixed(4);
  }

  // Fetch gas estimate for transaction
  async function fetchGasEstimate() {
    if (!isTauri || !$etcAccount || !isGethRunning) {
      gasError = '';
      return;
    }
    
    isLoadingGas = true;
    gasError = '';
    
    try {
      // Use a placeholder address if no recipient yet
      const toAddress = recipientAddress || '0x0000000000000000000000000000000000000000';
      const amount = sendAmount > 0 ? sendAmount : 0.001;
      
      const result = await invoke<GasEstimate>('estimate_transaction_gas', {
        from: $etcAccount.address,
        to: toAddress,
        value: amount
      });
      
      gasEstimate = result;
    } catch (error) {
      const errorMsg = String(error);
      // Only show error if it's not insufficient funds (which is expected for empty accounts)
      if (!errorMsg.includes('insufficient funds')) {
        console.error('Failed to fetch gas estimate:', error);
      }
      gasError = errorMsg;
      // Set default values if gas estimation fails
      gasEstimate = {
        gasLimit: 21000,
        gasPrices: {
          slow: { gwei: 1, fee: 0.000021, time: '~2 minutes' },
          standard: { gwei: 1.25, fee: 0.00002625, time: '~1 minute' },
          fast: { gwei: 1.5, fee: 0.0000315, time: '~30 seconds' }
        },
        networkCongestion: 'unknown'
      };
    } finally {
      isLoadingGas = false;
    }
  }

  // Refresh gas estimate when recipient or amount changes, or when geth starts
  $: if (isGethRunning && $etcAccount) {
    fetchGasEstimate();
  }

  // async function handleLogout() {
  //   if (isTauri) await invoke('logout');
  //   logout();
  // }
  
  // Locks account using shared helper (used by closing wizard and this page)
  async function handleLogout() {
    privateKeyVisible = false;
    try {
      await lockAccount();
    } catch (error) {
      console.error('Error during logout:', error);
    }
  }

  async function generateAndShowQrCode(){
    const address = $etcAccount?.address;
    if(!address) return;
    try{
      qrCodeDataUrl = await QRCode.toDataURL(address, {
        errorCorrectionLevel: 'high',
        type: 'image/png',
        width: 200,
        margin: 2,
        color: {
          dark: '#000000',
          light: '#FFFFFF'
        }
      });
      showQrCodeModal = true;
    }
    catch(err){
      console.error('Failed to generate QR code', err);
      alert('Could not generate the QR code.');
    }
  }

</script>

<div class="space-y-4 sm:space-y-6 px-2 sm:px-0">
  <div class="py-2 sm:py-0">
    <h1 class="text-2xl sm:text-3xl font-bold">{$t('account.title')}</h1>
    <p class="text-muted-foreground mt-1 sm:mt-2 text-sm sm:text-base">{$t('account.subtitle')}</p>
  </div>

  <!-- Warning Banner: Geth Not Running -->
  {#if $gethStatus !== 'running'}
    <div class="bg-yellow-500/10 border border-yellow-500/20 rounded-lg p-3 sm:p-4">
      <div class="flex items-start sm:items-center gap-2 sm:gap-3">
        <AlertCircle class="h-4 w-4 sm:h-5 sm:w-5 text-yellow-500 flex-shrink-0 mt-0.5 sm:mt-0" />
        <p class="text-xs sm:text-sm text-yellow-600 leading-relaxed">
          {$t('nav.blockchainUnavailable')} <button on:click={() => { navigation.setCurrentPage('network'); goto('/network'); }} class="underline font-medium hover:text-yellow-700 transition-colors">{$t('nav.networkPageLink')}</button>. {$t('account.balanceWarning')}
        </p>
      </div>
    </div>
  {/if}

{#if showMnemonicWizard}
  <MnemonicWizard
    mode={mnemonicMode}
    onCancel={closeMnemonicWizard}
    onComplete={completeMnemonicWizard}
  />
{/if}

  <div class="grid grid-cols-1 {$etcAccount ? 'lg:grid-cols-2' : ''} gap-3 sm:gap-4">
    <Card class="p-4 sm:p-6">
      <div class="flex items-center justify-between mb-3 sm:mb-4">
        <h2 class="text-base sm:text-lg font-semibold">{msg('wallet.title', 'Wallet')}</h2>
        <Wallet class="h-4 w-4 sm:h-5 sm:w-5 text-muted-foreground" />
      </div>
      
      <div class="space-y-3 sm:space-y-4">
        {#if !$etcAccount}
          <div class="space-y-3">
            <p class="text-sm text-muted-foreground">{msg('wallet.cta.intro', 'Create or import a wallet to get started.')}</p>
            
            <Button 
              class="w-full" 
              on:click={createChiralAccount}
              disabled={isCreatingAccount}
            >
              <Plus class="h-4 w-4 mr-2" />
              {isCreatingAccount ? $t('actions.creating') : $t('actions.createAccount')}
            </Button>
            <div class="flex flex-col sm:flex-row gap-2">
              <Button variant="outline" class="w-full sm:flex-1" on:click={openCreateMnemonic}>
                <KeyRound class="h-4 w-4 mr-1.5" /> 
                <span class="text-sm sm:text-base">{msg('wallet.hd.create_via_phrase', 'Create via recovery phrase')}</span>
              </Button>
              <Button variant="outline" class="w-full sm:flex-1" on:click={openImportMnemonic}>
                <Import class="h-4 w-4 mr-1.5" /> 
                <span class="text-sm sm:text-base">{msg('wallet.hd.import_phrase', 'Import recovery phrase')}</span>
              </Button>
            </div>
            
            <div class="space-y-2">
              <div class="flex flex-col sm:flex-row w-full gap-2 sm:gap-0">
                <Input
                  type="text"
                  bind:value={importPrivateKey}
                  placeholder={$t('placeholders.importPrivateKey')}
                  class="flex-1 sm:rounded-r-none sm:border-r-0 text-sm"
                  autocomplete="off"
                  data-form-type="other"
                  data-lpignore="true"
                  spellcheck="false"
                />
                <Button 
                  variant="outline"
                  size="default"
                  on:click={loadPrivateKeyFromFile}
                  class="w-full sm:w-auto sm:rounded-l-none sm:border-l-0 bg-gray-200 hover:bg-gray-300 border-gray-300 text-gray-900 shadow-sm text-sm"
                  title="Import private key from wallet JSON"
                >
                  <FileText class="h-4 w-4 mr-1.5 sm:mr-2" />
                  <span class="truncate">{msg('wallet.hd.load_from_wallet', 'Load from wallet')}</span>
                </Button>
              </div>
              <Button 
                class="w-full" 
                variant="outline"
                on:click={importChiralAccount}
                disabled={!importPrivateKey || isImportingAccount}
              >
                <Import class="h-4 w-4 mr-2" />
                {isImportingAccount ? $t('actions.importing') : $t('actions.importAccount')}
              </Button>
            </div>

            <div class="relative py-2">
              <div class="absolute inset-0 flex items-center">
                <span class="w-full border-t"></span>
              </div>
              <div class="relative flex justify-center text-xs uppercase">
                <span class="bg-card px-2 text-muted-foreground">{msg('wallet.cta.or', 'Or')}</span>
              </div>
            </div>

            <div class="space-y-3">
              <h3 class="text-md font-medium">{$t('keystore.load.title')}</h3>
              {#if keystoreAccounts.length > 0}
                <div class="space-y-2">
                  <div>
                    <Label for="keystore-account">{$t('keystore.load.select')}</Label>
                    <div class="mt-1">
                      <DropDown
                        id="keystore-account"
                        options={keystoreOptions}
                        bind:value={selectedKeystoreAccount}
                        disabled={keystoreAccounts.length === 0}
                      />
                    </div>
                  </div>
                  <div>
                    <Label for="keystore-password">{$t('placeholders.password')}</Label>
                    <Input
                      id="keystore-password"
                      type="password"
                      bind:value={loadKeystorePassword}
                      placeholder={$t('placeholders.unlockPassword')}
                      class="w-full mt-1"
                      autocomplete="current-password"
                    />
                  </div>
                  <div class="flex items-center space-x-2 mt-2">
                    <input type="checkbox" id="remember-password" bind:checked={rememberKeystorePassword} />
                    <label for="remember-password" class="text-sm font-medium leading-none text-muted-foreground cursor-pointer">
                      {$t('keystore.load.savePassword')}
                    </label>
                  </div>
                  {#if rememberKeystorePassword}
                    <div class="text-xs text-orange-600 p-2 bg-orange-50 border border-orange-200 rounded-md mt-2">
                      {$t('keystore.load.savePasswordWarning')}
                    </div>
                  {/if}
                  <Button
                    class="w-full"
                    variant="outline"
                    on:click={loadFromKeystore}
                    disabled={!selectedKeystoreAccount || !loadKeystorePassword || isLoadingFromKeystore}
                  >
                    <KeyRound class="h-4 w-4 mr-2" />
                    {isLoadingFromKeystore ? $t('actions.unlocking') : $t('actions.unlockAccount')}
                  </Button>
                  <Button
                    class="w-full mt-2"
                    variant="outline"
                    on:click={deleteKeystoreAccount}
                    disabled={!selectedKeystoreAccount || isLoadingFromKeystore}
                  >
                    <BadgeX class="h-4 w-4 mr-2 text-red-600" />
                    {$t('keystore.delete.button')}
                  </Button>
                  {#if keystoreLoadMessage}
                    <p class="text-xs text-center {keystoreLoadMessage.toLowerCase().includes('success') ? 'text-green-600' : 'text-red-600'}">{keystoreLoadMessage}</p>
                  {/if}
                </div>
              {:else}
                <p class="text-xs text-muted-foreground text-center py-2">{$t('keystore.load.empty')}</p>
              {/if}
            </div>

          </div>
        {:else}
        <div>
          <p class="text-sm text-muted-foreground">{msg('wallet.balance', 'Balance')}</p>
          <p class="text-3xl font-bold text-foreground">{$wallet.balance.toFixed(8)} Chiral</p>
        </div>
        
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-2 sm:gap-4 mt-3 sm:mt-4">
          <!-- Blocks Mined -->
          <div class="min-w-0 border border-gray-200 dark:border-gray-700 rounded-md p-2.5 sm:p-3 shadow-sm">
            <div class="flex items-center gap-1.5 sm:gap-2 mb-1.5 sm:mb-2">
              <div class="bg-purple-100 rounded p-1">
                <Coins class="h-3.5 w-3.5 sm:h-4 sm:w-4 text-purple-600" />
              </div>
            <p class="text-[10px] sm:text-xs text-muted-foreground truncate">Blocks Mined {#if !$accurateTotals}<span class="text-[10px] sm:text-xs opacity-60">(est.)</span>{/if}</p>
            </div>
            {#if $accurateTotals}
              <p class="text-sm sm:text-base font-semibold text-foreground break-words">{$accurateTotals.blocksMined.toLocaleString()} blocks</p>
            {:else}
              <p class="text-sm sm:text-base font-semibold text-foreground opacity-60 break-words">{$miningState.blocksFound.toLocaleString()} blocks</p>
            {/if}
          </div>
          <!-- Total Received -->
          <div class="min-w-0 border border-gray-200 dark:border-gray-700 rounded-md p-2.5 sm:p-3 shadow-sm">
            <div class="flex items-center gap-1.5 sm:gap-2 mb-1.5 sm:mb-2">
              <div class="bg-green-100 rounded p-1">
                <ArrowDownLeft class="h-3.5 w-3.5 sm:h-4 sm:w-4 text-green-600" />
              </div>
            <p class="text-[10px] sm:text-xs text-muted-foreground truncate">{msg('wallet.totalReceived', 'Total received')} {#if !$accurateTotals}<span class="text-[10px] sm:text-xs opacity-60">(est.)</span>{/if}</p>
            </div>
            {#if $accurateTotals}
              <p class="text-sm sm:text-base font-semibold text-foreground break-words">+{$accurateTotals.totalReceived.toFixed(8)}</p>
            {:else}
              <p class="text-sm sm:text-base font-semibold text-foreground opacity-60 break-words">+{$totalReceived.toFixed(8)}</p>
            {/if}
          </div>
          <!-- Total Spent -->
          <div class="min-w-0 border border-gray-200 dark:border-gray-700 rounded-md p-2.5 sm:p-3 shadow-sm">
            <div class="flex items-center gap-1.5 sm:gap-2 mb-1.5 sm:mb-2">
              <div class="bg-red-100 rounded p-1">
                <ArrowUpRight class="h-3.5 w-3.5 sm:h-4 sm:w-4 text-red-600" />
              </div>
            <p class="text-[10px] sm:text-xs text-muted-foreground truncate">{msg('wallet.totalSpent', 'Total spent')} {#if !$accurateTotals}<span class="text-[10px] sm:text-xs opacity-60">(est.)</span>{/if}</p>
            </div>
            {#if $accurateTotals}
              <p class="text-sm sm:text-base font-semibold text-foreground break-words">-{$accurateTotals.totalSent.toFixed(8)}</p>
            {:else}
              <p class="text-sm sm:text-base font-semibold text-foreground opacity-60 break-words">-{$totalSpent.toFixed(8)}</p>
            {/if}
          </div>
        </div>

        <!-- Accurate Totals Progress -->
        {#if $isCalculatingAccurateTotals}
          <div class="mt-4 space-y-2">
            <div class="flex items-center justify-between text-sm">
              <span class="text-muted-foreground">Calculating accurate totals...</span>
              {#if $accurateTotalsProgress}
                <span class="font-medium">{$accurateTotalsProgress.percentage}%</span>
              {/if}
            </div>
            {#if $accurateTotalsProgress}
              <Progress value={$accurateTotalsProgress.percentage} />
              <p class="text-xs text-muted-foreground">
                Block {$accurateTotalsProgress.currentBlock.toLocaleString()} / {$accurateTotalsProgress.totalBlocks.toLocaleString()}
              </p>
            {/if}
          </div>
        {:else if !$accurateTotals && accurateTotalsError}
          <div class="mt-4 flex items-center justify-between gap-3 text-sm">
            <span class="text-destructive truncate">Accurate totals failed: {accurateTotalsError}</span>
            <button
              on:click={calculateAccurateTotals}
              class="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 whitespace-nowrap"
              title="Retry accurate totals"
            >
              <RefreshCw class="h-3 w-3" />
              Retry
            </button>
          </div>
        {:else if $accurateTotals}
          <div class="mt-2 flex items-center justify-end">
            <button
              on:click={calculateAccurateTotals}
              class="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1"
              title="Recalculate accurate totals"
            >
              <RefreshCw class="h-3 w-3" />
              Refresh
            </button>
          </div>
        {/if}

            <div class="mt-4 sm:mt-6">
              <p class="text-xs sm:text-sm text-muted-foreground mb-1.5 sm:mb-1">{$t('wallet.address')}</p>
              <div class="flex items-center gap-1.5 sm:gap-2">
                <p class="font-mono text-xs sm:text-sm truncate flex-1">{$etcAccount.address.slice(0, 10)}...{$etcAccount.address.slice(-8)}</p>
                <Button size="sm" variant="outline" on:click={copyAddress} aria-label={$t('aria.copyAddress')} class="flex-shrink-0">
                  <Copy class="h-3 w-3" />
                </Button>
                <Button size="sm" variant="outline" on:click={generateAndShowQrCode} title={$t('tooltips.showQr')} aria-label={$t('aria.showQr')} class="flex-shrink-0">
                  <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5h3v3H5zM5 16h3v3H5zM16 5h3v3h-3zM16 16h3v3h-3zM10.5 5h3M10.5 19h3M5 10.5v3M19 10.5v3M10.5 10.5h3v3h-3z"/></svg>
                </Button>
                {#if showQrCodeModal}
                  <div
                    class="fixed inset-0 bg-black/50 backdrop-blur-md flex items-center justify-center z-50 p-3 sm:p-4 animate-in fade-in duration-200"
                    role="button"
                    tabindex="0"
                    on:click={() => showQrCodeModal = false}
                    on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') showQrCodeModal = false; }}
                  >
                    <div
                      class="bg-white p-5 sm:p-8 rounded-2xl shadow-[0_20px_60px_rgba(0,0,0,0.3)] w-full max-w-md text-center border border-purple-200 animate-in zoom-in-95 duration-200 max-h-[90vh] overflow-y-auto"
                      on:click|stopPropagation
                      role="dialog"
                      tabindex="0"
                      aria-modal="true"
                      on:keydown={(e) => { if (e.key === 'Escape') showQrCodeModal = false; }}
                    >
                      <!-- Header -->
                      <div class="flex items-center justify-between mb-4 sm:mb-6">
                        <div class="flex items-center gap-2 sm:gap-3">
                          <div class="bg-purple-100 p-1.5 sm:p-2.5 rounded-xl shadow-sm">
                            <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-purple-600 sm:w-[22px] sm:h-[22px]"><path d="M5 5h3v3H5zM5 16h3v3H5zM16 5h3v3h-3zM16 16h3v3h-3zM10.5 5h3M10.5 19h3M5 10.5v3M19 10.5v3M10.5 10.5h3v3h-3z"/></svg>
                          </div>
                          <h3 class="text-lg sm:text-2xl font-bold text-gray-900">{msg('wallet.qrModal.title', 'Your wallet address')}</h3>
                        </div>
                        <button
                          on:click={() => showQrCodeModal = false}
                          class="text-gray-400 hover:text-gray-600 transition-colors p-1 rounded-lg hover:bg-gray-100 flex-shrink-0"
                          aria-label="Close"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" class="sm:w-6 sm:h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
                        </button>
                      </div>
                      
                      <!-- QR Code -->
                      <div class="bg-white p-4 sm:p-6 rounded-2xl border-2 border-purple-200 shadow-lg inline-block mb-4 sm:mb-6 transition-transform hover:scale-105 duration-200">
                        <img src={qrCodeDataUrl} alt={msg('wallet.qrModal.alt', 'Wallet address QR code')} class="mx-auto rounded-lg w-full max-w-[200px] sm:max-w-none" />
                      </div>
                      
                      <!-- Address -->
                      <div class="bg-gray-50 border border-gray-200 rounded-xl p-3 sm:p-4 mb-4 sm:mb-6">
                        <p class="text-[10px] sm:text-xs text-gray-500 mb-1 font-medium">Wallet Address</p>
                        <p class="text-xs sm:text-sm text-gray-800 break-all font-mono leading-relaxed">
                          {$etcAccount?.address}
                        </p>
                      </div>

                      <Button 
                        variant="outline"
                        class="w-full font-semibold hover:bg-gray-100 transition-all duration-200 text-sm sm:text-base" 
                        on:click={() => showQrCodeModal = false}
                      >
                        {$t('actions.close')}
                      </Button>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
            
            <div class="mt-4">
              <p class="text-sm text-muted-foreground">{msg('wallet.privateKey', 'Private key')}</p>
                <div class="flex items-center gap-2 mt-1">
                  <Input
                    type="text"
                    value={privateKeyVisible ? $etcAccount.private_key : '•'.repeat($etcAccount.private_key.length)}
                    readonly
                    class="flex-1 font-mono text-xs min-w-0 h-9"
                  />
                <Button
                  size="sm"
                  variant="outline"
                  on:click={copyPrivateKey}
                  aria-label={$t('aria.copyPrivateKey')}
                  class="h-9 px-3"
                >
                  <Copy class="h-3 w-3" />
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  class="w-16 h-9 px-3"
                  on:click={togglePrivateKeyVisibility}
                >
                  {privateKeyVisible ? $t('actions.hide') : $t('actions.show')}
                </Button>
              </div>
               <p class="text-xs text-muted-foreground mt-1">{$t('warnings.neverSharePrivateKey')}</p>
             </div>
             
            <div class="mt-6 space-y-2">
              <div class="grid grid-cols-2 gap-2">
                <Button type="button" variant="outline" on:click={exportWallet}>
                  {msg('wallet.export', 'Export wallet')}
                </Button>
                <Button type="button" variant="destructive" on:click={handleLogout}>
                  {$t('actions.lockWallet')}
                </Button>
              </div>
              {#if exportMessage}<p class="text-xs text-center mt-2 {exportMessage.includes('successfully') ? 'text-green-600' : 'text-red-600'}">{exportMessage}</p>{/if}
            </div>
         {/if}
      </div>
    </Card>
    
    {#if $etcAccount}
    <Card class="p-4 sm:p-6">
    <div class="flex items-center justify-between mb-3 sm:mb-4">
      <h2 class="text-base sm:text-lg font-semibold">{$t('transfer.title')}</h2>
      <Coins class="h-4 w-4 sm:h-5 sm:w-5 text-muted-foreground" />
    </div>
    <form autocomplete="off" data-form-type="other" data-lpignore="true">
      <div class="space-y-4">
        <div>
          <Label for="recipient">{$t('transfer.recipient.label')}</Label>
          <div class="relative mt-2">
            <Input
              id="recipient"
              bind:value={recipientAddress}
              placeholder={$t('transfer.recipient.placeholder')}
              class="pr-20 {isAddressValid ? 'border-green-500 focus:ring-green-500' : recipientAddress && !isAddressValid ? 'border-red-500 focus:ring-red-500' : ''}" 
              data-form-type="other"
              data-lpignore="true"
              aria-autocomplete="none"
            />
            {#if isAddressValid}
              <div class="absolute right-12 top-1/2 -translate-y-1/2 text-green-600">
                <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
              </div>
            {/if}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              class="absolute right-1 top-1/2 -translate-y-1/2 h-8 w-8 p-0"
              on:click={scanQrCode}
              aria-label={$t('transfer.recipient.scanQr')}
              title={$t('transfer.recipient.scanQr')}
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"></rect><rect x="14" y="3" width="7" height="7"></rect><rect x="3" y="14" width="7" height="7"></rect><line x1="14" x2="14" y1="14" y2="21"></line><line x1="21" x2="21" y1="14" y2="21"></line><line x1="21" x2="14" y1="21" y2="21"></line></svg>
            </Button>
          </div>
          {#if showScannerModal}
            <div class="fixed inset-0 bg-black/50 backdrop-blur-md flex items-center justify-center z-50 p-3 sm:p-4 animate-in fade-in duration-200">
              <div class="bg-white dark:bg-gray-900 p-4 sm:p-6 rounded-2xl shadow-[0_20px_60px_rgba(0,0,0,0.3)] w-full max-w-md border border-purple-200 dark:border-purple-800 animate-in zoom-in-95 duration-200 max-h-[90vh] overflow-y-auto">
                <!-- Header -->
                <div class="flex items-center justify-between mb-4 sm:mb-5">
                  <div class="flex items-center gap-2 sm:gap-3">
                    <div class="bg-purple-100 p-1.5 sm:p-2.5 rounded-xl shadow-sm">
                      <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-purple-600 sm:w-[22px] sm:h-[22px]"><path d="M5 5h3v3H5zM5 16h3v3H5zM16 5h3v3h-3zM16 16h3v3h-3zM10.5 5h3M10.5 19h3M5 10.5v3M19 10.5v3M10.5 10.5h3v3h-3z"/></svg>
                    </div>
                    <h3 class="text-lg sm:text-2xl font-bold text-gray-900 dark:text-white">{$t('transfer.recipient.scanQrTitle')}</h3>
                  </div>
                  <button
                    on:click={() => showScannerModal = false}
                    class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 flex-shrink-0"
                    aria-label="Close"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" class="sm:w-6 sm:h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
                  </button>
                </div>
                
                <!-- Scanner View -->
                <div id="qr-reader" class="w-full border-2 border-purple-300 rounded-xl overflow-hidden shadow-inner mb-3 sm:mb-4 bg-gray-50 dark:bg-gray-800"></div>
                
                <div class="flex gap-2">
                  <Button 
                    class="flex-1 font-semibold bg-purple-600 hover:bg-purple-700 text-white shadow-md hover:shadow-lg transition-all duration-200 text-sm sm:text-base" 
                    on:click={() => showScannerModal = false}
                  >
                    {$t('actions.cancel')}
                  </Button>
                </div>
              </div>
            </div>
          {/if}
          <div class="flex items-center justify-between mt-1">
            <span class="text-xs text-muted-foreground">
              {recipientAddress.length}/42 {$t('transfer.recipient.characters')}
              {#if recipientAddress.length <= 42}
                ({42 - recipientAddress.length} {$t('transfer.recipient.remaining')})
              {:else}
                ({recipientAddress.length - 42} {$t('transfer.recipient.over')})
              {/if}
            </span>
            {#if addressWarning}
              <p class="text-xs text-red-500 font-medium">{addressWarning}</p>
            {/if}
          </div>
        </div>

        <div>
          <Label for="amount">{$t('transfer.amount.label')}</Label>
          <div class="relative mt-2">
            <Input
              id="amount"
              type="text"
              inputmode="decimal"
              bind:value={rawAmountInput}
              placeholder="0.00"
              class="{isAmountValid ? 'border-green-500 focus:ring-green-500' : rawAmountInput && !isAmountValid ? 'border-red-500 focus:ring-red-500' : ''}"
              data-form-type="other"
              data-lpignore="true"
              aria-autocomplete="none"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              class="absolute right-1 top-1/2 transform -translate-y-1/2 h-8 px-3 text-xs"
              on:click={setMaxAmount}
              disabled={$wallet.balance <= 0}
            >
              {$t('transfer.amount.max')}
            </Button>
          </div>
          <div class="flex items-center justify-between mt-1">
            <p class="text-xs text-muted-foreground">
              {$t('transfer.available', { values: { amount: $wallet.balance.toFixed(4) } })}
            </p>
            {#if validationWarning}
              <p class="text-xs text-red-500 font-medium">{validationWarning}</p>
            {/if}
          </div>
          
          <!-- Fee selector -->
          <div class="mt-3">
            <Label class="text-xs mb-2 block">{$t('transfer.fees.estimated')}</Label>
            <div class="inline-flex rounded-md border border-gray-300 dark:border-gray-600 overflow-hidden">
              <button 
                type="button" 
                class="px-4 py-2 text-xs font-medium transition-colors {selectedGasOption === 'slow' ? 'bg-gray-800 text-white' : 'bg-white text-gray-700 hover:bg-gray-50'}" 
                on:click={() => selectedGasOption = 'slow'}
              >
                {$t('transfer.fees.low')}
              </button>
              <button 
                type="button" 
                class="px-4 py-2 text-xs font-medium border-l border-gray-300 dark:border-gray-600 transition-colors {selectedGasOption === 'standard' ? 'bg-gray-800 text-white' : 'bg-white text-gray-700 hover:bg-gray-50'}" 
                on:click={() => selectedGasOption = 'standard'}
              >
                {$t('transfer.fees.market')}
              </button>
              <button 
                type="button" 
                class="px-4 py-2 text-xs font-medium border-l border-gray-300 dark:border-gray-600 transition-colors {selectedGasOption === 'fast' ? 'bg-gray-800 text-white' : 'bg-white text-gray-700 hover:bg-gray-50'}" 
                on:click={() => selectedGasOption = 'fast'}
              >
                {$t('transfer.fees.fast')}
              </button>
            </div>
            <p class="text-xs text-muted-foreground mt-2">
              <span class="font-medium">Fee:</span> {estimatedFeeDisplay}
            </p>
          </div>
        
        </div>

        <!-- Gas Options Section -->
        <div class="space-y-2">
          <div class="flex items-center justify-between">
            <Label>{$t('transfer.gas.label')}</Label>
            {#if isLoadingGas}
              <span class="text-xs text-muted-foreground flex items-center gap-1">
                <RefreshCw class="h-3 w-3 animate-spin" />
                {$t('transfer.gas.loading')}
              </span>
            {:else if gasError}
              <span class="text-xs text-amber-500">{$t('transfer.gas.estimateError')}</span>
            {/if}
          </div>
          
          <div class="grid grid-cols-3 gap-2">
            <!-- Slow Option -->
            <button
              type="button"
              class="p-3 rounded-lg border-2 transition-all text-left {selectedGasOption === 'slow' ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20' : 'border-gray-200 dark:border-gray-700 hover:border-gray-300'}"
              on:click={() => selectedGasOption = 'slow'}
            >
              <div class="flex items-center gap-1 mb-1">
                <span class="text-lg">🐢</span>
                <span class="text-sm font-medium">{$t('transfer.gas.slow')}</span>
              </div>
              {#if gasEstimate}
                <p class="text-xs text-muted-foreground">{gasEstimate.gasPrices.slow.gwei.toFixed(2)} Gwei</p>
                <p class="text-xs font-medium text-green-600">{gasEstimate.gasPrices.slow.fee.toFixed(6)} CHR</p>
                <p class="text-xs text-muted-foreground">{gasEstimate.gasPrices.slow.time}</p>
              {:else}
                <p class="text-xs text-muted-foreground">--</p>
              {/if}
            </button>

            <!-- Standard Option -->
            <button
              type="button"
              class="p-3 rounded-lg border-2 transition-all text-left {selectedGasOption === 'standard' ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20' : 'border-gray-200 dark:border-gray-700 hover:border-gray-300'}"
              on:click={() => selectedGasOption = 'standard'}
            >
              <div class="flex items-center gap-1 mb-1">
                <span class="text-lg">⚡</span>
                <span class="text-sm font-medium">{$t('transfer.gas.standard')}</span>
              </div>
              {#if gasEstimate}
                <p class="text-xs text-muted-foreground">{gasEstimate.gasPrices.standard.gwei.toFixed(2)} Gwei</p>
                <p class="text-xs font-medium text-blue-600">{gasEstimate.gasPrices.standard.fee.toFixed(6)} CHR</p>
                <p class="text-xs text-muted-foreground">{gasEstimate.gasPrices.standard.time}</p>
              {:else}
                <p class="text-xs text-muted-foreground">--</p>
              {/if}
            </button>

            <!-- Fast Option -->
            <button
              type="button"
              class="p-3 rounded-lg border-2 transition-all text-left {selectedGasOption === 'fast' ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20' : 'border-gray-200 dark:border-gray-700 hover:border-gray-300'}"
              on:click={() => selectedGasOption = 'fast'}
            >
              <div class="flex items-center gap-1 mb-1">
                <span class="text-lg">🚀</span>
                <span class="text-sm font-medium">{$t('transfer.gas.fast')}</span>
              </div>
              {#if gasEstimate}
                <p class="text-xs text-muted-foreground">{gasEstimate.gasPrices.fast.gwei.toFixed(2)} Gwei</p>
                <p class="text-xs font-medium text-orange-600">{gasEstimate.gasPrices.fast.fee.toFixed(6)} CHR</p>
                <p class="text-xs text-muted-foreground">{gasEstimate.gasPrices.fast.time}</p>
              {:else}
                <p class="text-xs text-muted-foreground">--</p>
              {/if}
            </button>
          </div>

          <!-- Estimated Fee Summary -->
          {#if gasEstimate}
            <div class="flex items-center justify-between p-2 bg-blue-50 dark:bg-blue-900/30 rounded-lg border border-blue-200 dark:border-blue-700">
              <span class="text-sm text-gray-700 dark:text-gray-200">{$t('transfer.gas.estimatedFee')}</span>
              <span class="text-sm font-medium text-gray-900 dark:text-white">
                {gasEstimate.gasPrices[selectedGasOption].fee.toFixed(6)} CHR
              </span>
            </div>
            {#if sendAmount > 0}
              <div class="flex items-center justify-between p-2 bg-green-50 dark:bg-green-900/30 rounded-lg border border-green-200 dark:border-green-700">
                <span class="text-sm text-gray-700 dark:text-gray-200">{$t('transfer.gas.totalCost')}</span>
                <span class="text-sm font-bold text-gray-900 dark:text-white">
                  {(sendAmount + gasEstimate.gasPrices[selectedGasOption].fee).toFixed(6)} CHR
                </span>
              </div>
            {/if}
          {/if}
        </div>

        <Button
          type="button"
          class="w-full font-semibold transition-all {isConfirming ? 'bg-orange-600 hover:bg-orange-700' : ''}"
          on:click={handleSendClick}
          disabled={!isAddressValid || !isAmountValid || rawAmountInput === ''}>
          {#if isConfirming}
            <div class="flex items-center justify-center gap-2">
              <div class="animate-spin rounded-full h-4 w-4 border-2 border-white border-t-transparent"></div>
            {$t('transfer.sendingIn', { values: { seconds: countdown } })}
            </div>
          {:else}
            <div class="flex items-center justify-center gap-2">
              <ArrowUpRight class="h-4 w-4" />
            {$t('transfer.send')}
            </div>
          {/if}
        </Button>

        <Button type="button" variant="outline" class="w-full justify-center bg-white dark:bg-gray-800 border border-orange-200 dark:border-orange-800 hover:bg-orange-50 dark:hover:bg-orange-950/30 text-gray-800 dark:text-gray-200 rounded transition-colors py-2 font-medium" on:click={() => showPending = !showPending} aria-label={$t('transfer.viewPending')}>
          <span class="flex items-center gap-2">
            <div class="relative">
              <svg class="h-4 w-4 text-orange-600 dark:text-orange-500" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24">
              <circle cx="12" cy="10" r="8" />
              <polyline points="12,6 12,10 16,14" />
            </svg>
              {#if $pendingCount > 0}
                <span class="absolute -top-1 -right-1 bg-orange-600 text-white text-[9px] font-bold rounded-full h-3.5 w-3.5 flex items-center justify-center">{$pendingCount}</span>
              {/if}
            </div>
            {$t('transfer.pending.count', { values: { count: $pendingCount } })}
          </span>
        </Button>
        {#if showPending}
          <div class="mt-2 p-3 border border-gray-200 dark:border-gray-700 rounded-md shadow-sm">
            <h3 class="text-sm mb-2 text-foreground font-semibold flex items-center gap-2">
              <History class="h-4 w-4 text-orange-600" />
              {$t('transfer.pending.title')}
            </h3>
            <ul class="space-y-2">
              {#each $transactions.filter(tx => tx.status === 'pending') as tx}
                <li class="text-xs border border-orange-200 dark:border-orange-800 rounded p-2">
                  <div class="flex items-start gap-2">
                    <span class="bg-orange-600 text-white text-[9px] px-1.5 py-0.5 rounded font-semibold">PENDING</span>
                    <div class="flex-1 min-w-0">
                      <div class="font-medium text-foreground">{tx.description}</div>
                      <div class="text-muted-foreground truncate mt-0.5">
                        {tx.type === 'sent' ? $t('transactions.item.to') : $t('transactions.item.from')}: {tx.type === 'sent' ? tx.to : tx.from}
                      </div>
                      <div class="font-semibold text-orange-700 dark:text-orange-500 mt-0.5">{tx.amount} Chiral</div>
                    </div>
                  </div>
                </li>
              {:else}
                <li class="text-xs text-center py-2 text-muted-foreground">{$t('transfer.pending.noDetails')}</li>
              {/each}
            </ul>
          </div>
        {/if}
        </div>
      </form>
  </Card>
  {/if}


  </div>

  {#if $etcAccount}
    {#if hdMnemonic}
        <Card class="p-6">
          <div class="flex items-center justify-between mb-4">
            <h2 class="text-lg font-semibold">HD Wallet</h2>
            <div class="flex gap-2">
              <Button variant="outline" on:click={openCreateMnemonic}>New</Button>
              <Button variant="outline" on:click={openImportMnemonic}>Import</Button>
            </div>
          </div>
          <p class="text-sm text-muted-foreground mb-4">Path m/44'/{chainId}'/0'/0/*</p>
          <AccountList
            mnemonic={hdMnemonic}
            passphrase={hdPassphrase}
            accounts={hdAccounts}
            onAccountsChange={onHDAccountsChange}
          />
        </Card>
    {/if}
  <!-- Transaction History Section - Full Width -->
  <Card class="p-4 sm:p-6 mt-3 sm:mt-4">
    <div class="flex items-center justify-between mb-2 sm:mb-2">
      <h2 class="text-base sm:text-lg font-semibold">{$t('transactions.title')}</h2>
      <History class="h-4 w-4 sm:h-5 sm:w-5 text-muted-foreground" />
    </div>

    <!-- Scan Range Info -->
    <p class="text-xs text-muted-foreground mb-4">
      {$t('transactions.scanInfo')}
    </p>

    <!-- Search Bar -->
    <div class="mb-4">
      <div class="relative">
        <svg class="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"></path>
        </svg>
        <input
          type="text"
          bind:value={searchQuery}
          placeholder={tr('transactions.searchPlaceholder')}
          class="w-full pl-10 pr-10 py-2.5 border border-gray-300 dark:border-gray-600 rounded-lg text-sm bg-white focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
        />
        {#if searchQuery}
          <button
            type="button"
            class="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 p-1 rounded-full hover:bg-gray-100 transition-colors"
            on:click={() => searchQuery = ''}
            title={$t('transactions.clearSearch')}
            aria-label={$t('transactions.clearSearch')}
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
          </button>
        {/if}
      </div>
    </div>

    <!-- Transaction Hash Search -->
    <div class="mb-4 p-4 border border-gray-200 dark:border-gray-700 rounded-lg" style="background-color: white;">
      <label for="tx-hash-search" class="block text-xs font-semibold mb-2 text-foreground">
        Search by Transaction Hash
      </label>
      <div class="flex gap-2">
        <input
          id="tx-hash-search"
          type="text"
          bind:value={txHashSearch}
          placeholder="0x..."
          class="flex-1 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md text-sm bg-white focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
        />
        <button
          type="button"
          class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          on:click={searchByTransactionHash}
          disabled={isSearchingHash || !txHashSearch}
        >
          {isSearchingHash ? 'Searching...' : 'Search'}
        </button>
        {#if txHashSearch}
          <button
            type="button"
            class="px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-md hover:bg-gray-100 transition-colors"
            on:click={() => { txHashSearch = ''; hashSearchResult = null; hashSearchError = ''; }}
          >
            Clear
          </button>
        {/if}
      </div>
      {#if hashSearchError}
        <p class="text-xs text-red-600 mt-2">{hashSearchError}</p>
      {/if}
      {#if hashSearchResult}
        <div class="mt-3 p-3 border border-green-300 dark:border-green-700 rounded bg-white dark:bg-gray-800">
          <p class="text-xs font-semibold text-green-700 dark:text-green-400 mb-2">Transaction Found</p>
          <p class="text-xs"><span class="font-semibold">Hash:</span> {hashSearchResult.hash || 'N/A'}</p>
          <p class="text-xs"><span class="font-semibold">From:</span> {hashSearchResult.from || 'N/A'}</p>
          <p class="text-xs"><span class="font-semibold">To:</span> {hashSearchResult.to || 'N/A'}</p>
          <p class="text-xs"><span class="font-semibold">Value:</span> {hashSearchResult.value || 'N/A'} CHR</p>
          <p class="text-xs"><span class="font-semibold">Block:</span> {hashSearchResult.blockNumber ?? hashSearchResult.block_number ?? 'Pending'}</p>
        </div>
      {/if}
    </div>

    <!-- Filters -->
    <div class="flex flex-wrap gap-3 mb-4 items-end p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
  <div>
    <label for="filter-type" class="block text-xs font-semibold mb-1.5 text-foreground">
      {$t('filters.type')}
    </label>
    <div class="relative">
      <select
        id="filter-type"
        bind:value={filterType}
        class="appearance-none border border-gray-300 dark:border-gray-600 rounded-md pl-3 pr-10 py-2 text-sm h-9 bg-white cursor-pointer hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
      >
        <option value="transactions">{$t('filters.typeTransactions')}</option>
        <option value="sent">{$t('filters.typeSent')}</option>
        <option value="received">{$t('filters.typeReceived')}</option>
        <option value="mining">{$t('filters.typeMining')}</option>
      </select>
      <div class="pointer-events-none absolute inset-y-0 right-0 flex items-center px-2 text-gray-500">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path></svg>
      </div>
    </div>
  </div>

  <div>
    <label for="filter-date-from" class="block text-xs font-semibold mb-1.5 text-foreground">
      {$t('filters.from')}
    </label>
    <div class="relative">
    <input
      id="filter-date-from"
      type="date"
      bind:value={filterDateFrom}
        class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 bg-white hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
    />
    </div>
  </div>

  <div>
    <label for="filter-date-to" class="block text-xs font-semibold mb-1.5 text-foreground">
      {$t('filters.to')}
    </label>
    <div class="relative">
    <input
      id="filter-date-to"
      type="date"
      bind:value={filterDateTo}
        class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 bg-white hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
    />
    </div>
  </div>

  <div>
    <label for="sort-button" class="block text-xs font-semibold mb-1.5 text-foreground">
      {$t('filters.sort')}
    </label>
    <button
      id="sort-button"
      type="button"
      class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 bg-white hover:border-gray-400 hover:bg-gray-50 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 w-full text-left transition-colors flex items-center gap-2"
      on:click={() => { sortDescending = !sortDescending; }}
      aria-pressed={sortDescending}
    >
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-gray-500"><path d="m3 16 4 4 4-4"></path><path d="M7 20V4"></path><path d="m21 8-4-4-4 4"></path><path d="M17 4v16"></path></svg>
      {sortDescending ? $t('filters.sortNewest') : $t('filters.sortOldest')}
    </button>
  </div>

  <div>
    <label for="min-amount" class="block text-xs font-semibold mb-1.5 text-foreground">
      Min Amount (CHR)
    </label>
    <input
      id="min-amount"
      type="number"
      bind:value={minAmount}
      min="0"
      step="0.01"
      placeholder="0"
      class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 w-28 bg-white hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
    />
  </div>

  <div>
    <label for="max-amount" class="block text-xs font-semibold mb-1.5 text-foreground">
      Max Amount (CHR)
    </label>
    <input
      id="max-amount"
      type="number"
      bind:value={maxAmount}
      min="0"
      step="0.01"
      placeholder="∞"
      class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 w-28 bg-white hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
    />
  </div>

  <div>
    <label for="min-gas" class="block text-xs font-semibold mb-1.5 text-foreground">
      Min Gas (Gwei)
    </label>
    <input
      id="min-gas"
      type="number"
      bind:value={minGasPrice}
      min="0"
      step="0.1"
      placeholder="0"
      class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 w-28 bg-white hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
    />
  </div>

  <div>
    <label for="max-gas" class="block text-xs font-semibold mb-1.5 text-foreground">
      Max Gas (Gwei)
    </label>
    <input
      id="max-gas"
      type="number"
      bind:value={maxGasPrice}
      min="0"
      step="0.1"
      placeholder="∞"
      class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 w-28 bg-white hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
    />
  </div>

  <div>
    <label for="block-number" class="block text-xs font-semibold mb-1.5 text-foreground">
      Block Number
    </label>
    <input
      id="block-number"
      type="text"
      bind:value={blockNumberSearch}
      placeholder="Block #"
      class="border border-gray-300 dark:border-gray-600 rounded-md px-3 py-2 text-sm h-9 w-28 bg-white hover:border-gray-400 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 transition-colors"
    />
  </div>

  <div class="flex-1"></div>

  <div class="flex flex-col gap-1 items-end">
    <div class="flex gap-2">
      <button
        type="button"
        class="border border-blue-300 dark:border-blue-700 rounded-md px-4 py-2 text-sm h-9 bg-blue-50 hover:bg-blue-100 text-blue-700 dark:text-blue-400 transition-colors font-medium flex items-center gap-2 {filteredTransactions.length === 0 ? 'opacity-50 cursor-not-allowed' : ''}"
        on:click={exportTransactionsCSV}
        title={filteredTransactions.length === 0 ? 'No transactions to export' : $t('transactions.exportCSV')}
      >
        <Download class="h-3.5 w-3.5" />
        {$t('transactions.export')}
      </button>
      <button
        type="button"
        class="border border-red-300 dark:border-red-700 rounded-md px-4 py-2 text-sm h-9 bg-red-50 hover:bg-red-100 text-red-700 dark:text-red-400 transition-colors font-medium flex items-center gap-2"
        on:click={() => {
          filterType = 'transactions';
          filterDateFrom = '';
          filterDateTo = '';
          sortDescending = true;
          searchQuery = '';
          txHashSearch = '';
          minAmount = 0;
          maxAmount = null;
          blockNumberSearch = '';
          minGasPrice = 0;
          maxGasPrice = null;
          hashSearchResult = null;
          hashSearchError = '';
        }}
      >
        <RefreshCw class="h-3.5 w-3.5" />
        {$t('filters.reset')}
      </button>
    </div>
  </div>
</div>

    <!-- Transaction List -->
    <div class="space-y-2 max-h-80 overflow-y-auto pr-1">
      {#each filteredTransactions as tx}
        <div 
          class="flex items-center justify-between p-3 border rounded-lg cursor-pointer transition-all hover:shadow-md {tx.type === 'mining' ? 'border-l-4 border-l-yellow-500 border-y border-r border-gray-200 dark:border-gray-700' : tx.type === 'received' ? 'border-l-4 border-l-green-500 border-y border-r border-gray-200 dark:border-gray-700' : 'border-l-4 border-l-red-500 border-y border-r border-gray-200 dark:border-gray-700'}"
          on:click={() => handleTransactionClick(tx)}
          on:keydown={(e) => {
            if (e.key === 'Enter') {
              handleTransactionClick(tx)
            }
          }}
          role="button"
          tabindex="0"
          in:fly={{ y: 20, duration: 300 }}
          out:fade={{ duration: 200 }}
        >
          <div class="flex items-center gap-3 flex-1 min-w-0">
            <div class="flex-shrink-0 {tx.type === 'mining' ? 'bg-yellow-100' : tx.type === 'received' ? 'bg-green-100' : 'bg-red-100'} p-2 rounded">
              {#if tx.type === 'mining'}
                <Coins class="h-4 w-4 text-yellow-600" />
              {:else if tx.type === 'received'}
                <ArrowDownLeft class="h-4 w-4 text-green-600" />
            {:else}
                <ArrowUpRight class="h-4 w-4 text-red-600" />
            {/if}
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 mb-1">
                <p class="text-sm font-medium truncate">{tx.description}</p>
                <span class="flex-shrink-0 text-[10px] px-2 py-0.5 rounded font-semibold {tx.type === 'mining' ? 'bg-yellow-500 text-white' : tx.type === 'received' ? 'bg-green-500 text-white' : 'bg-red-500 text-white'}">
                  {tx.type === 'sent' ? 'SENT' : tx.type === 'received' ? 'RECEIVED' : 'MINING'}
                </span>
              </div>
              <p class="text-xs text-muted-foreground truncate">
                {tx.type === 'received' ? $t('transactions.item.from') : $t('transactions.item.to')}: {tx.type === 'received' ? tx.from : tx.to}
              </p>
            </div>
          </div>
          <div class="text-right flex-shrink-0 ml-2">
            <p class="text-sm font-semibold {tx.type === 'received' || tx.type === 'mining' ? 'text-green-600' : 'text-red-600'}">
              {tx.type === 'received' || tx.type === 'mining' ? '+' : '-'}{tx.amount} Chiral
            </p>
            <p class="text-xs text-muted-foreground">{formatDate(tx.date)}</p>
          </div>
        </div>
      {/each}

      <!-- Loading Progress Indicators -->
      {#if filteredTransactions.length > 0}
        <div class="border-t">
          <!-- Transaction Auto-Loading Progress -->
          {#if $transactionPagination.isLoading}
            <div class="text-center py-3">
              <div class="flex items-center justify-center gap-2">
                <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-blue-500"></div>
                <p class="text-sm text-muted-foreground">{$t('transactions.loadingHistory')}</p>
              </div>
              {#if $transactionPagination.oldestBlockScanned !== null}
                <p class="text-xs text-muted-foreground mt-1">
                  {$t('transactions.scannedUpTo', { values: { block: $transactionPagination.oldestBlockScanned } })}
                </p>
              {/if}
            </div>
          {:else if !$transactionPagination.hasMore}
            <div class="text-center py-3">
              <p class="text-sm text-green-600">✓ All transactions loaded</p>
              {#if $transactionPagination.oldestBlockScanned !== null}
                <p class="text-xs text-muted-foreground mt-1">
                  Scanned all blocks from #{$transactionPagination.oldestBlockScanned.toLocaleString()} to current
                </p>
              {/if}
            </div>
          {:else if $transactionPagination.hasMore && $transactionPagination.oldestBlockScanned !== null && filterType !== 'mining'}
            <!-- Manual Load More Button for Regular Transactions -->
            <div class="text-center py-3 border-t">
              <Button
                on:click={() => walletService.loadMoreTransactions()}
                disabled={$transactionPagination.isLoading}
                variant="outline"
                class="gap-2"
              >
                {#if $transactionPagination.isLoading}
                  <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-blue-500"></div>
                  Loading Transactions...
                {:else}
                  <History class="w-4 h-4" />
                  Load More Transactions
                {/if}
              </Button>
              <p class="text-xs text-muted-foreground mt-2">
                Scanned up to block #{$transactionPagination.oldestBlockScanned.toLocaleString()}
              </p>
            </div>
          {/if}

          <!-- Mining Rewards Manual Loading - Only show when filterType is 'mining' -->
          {#if filterType === 'mining'}
            {#if $miningPagination.hasMore && $miningPagination.oldestBlockScanned !== null}
              <div class="text-center py-3 border-t">
                <Button
                  on:click={() => walletService.loadMoreMiningRewards()}
                  disabled={$miningPagination.isLoading}
                  variant="outline"
                  class="gap-2"
                >
                  {#if $miningPagination.isLoading}
                    <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-blue-500"></div>
                    Loading Mining Rewards...
                  {:else}
                    <Coins class="w-4 h-4" />
                    Load More Mining Rewards
                  {/if}
                </Button>
                <p class="text-xs text-muted-foreground mt-2">
                  {$t('transactions.miningRewards.scannedUpTo', { block: $miningPagination.oldestBlockScanned.toLocaleString() })}
                </p>
              </div>
            {:else if !$miningPagination.hasMore && $miningPagination.oldestBlockScanned !== null}
              <div class="text-center py-3 border-t">
                <p class="text-sm text-green-600">✓ {$t('transactions.miningRewards.allLoaded')}</p>
                <p class="text-xs text-muted-foreground mt-1">
                  {$t('transactions.miningRewards.scannedAll')}
                </p>
              </div>
            {/if}
          {/if}
        </div>
      {/if}

      {#if filteredTransactions.length === 0}
        <div class="text-center py-8 text-muted-foreground">
          <History class="h-12 w-12 mx-auto mb-2 opacity-20" />
          <p>{$t('transactions.empty.title')}</p>
          <p class="text-sm mt-1">{$t('transactions.empty.desc')}</p>
        </div>
      {/if}
    </div>
  </Card>
  {/if}

  {#if $etcAccount}
  <Card class="p-4 sm:p-6">
      <div class="flex items-start sm:items-center justify-between mb-3 sm:mb-4 gap-2">
        <div class="flex items-start sm:items-center gap-2 sm:gap-3">
          <div class="bg-blue-100 p-1.5 sm:p-2 rounded-lg flex-shrink-0">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-blue-600 w-[18px] h-[18px] sm:w-5 sm:h-5"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>
          </div>
        <div>
          <h2 class="text-base sm:text-lg font-semibold">{$t('security.2fa.title')}</h2>
            <p class="text-xs sm:text-sm text-muted-foreground">{$t('security.2fa.subtitle_clear')}</p>
        </div>
        </div>
      </div>
      <div class="space-y-3 sm:space-y-4">
        {#if is2faEnabled}
          <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 p-3 sm:p-4 bg-green-50 border-l-4 border-l-green-500 border-y border-r border-green-200 rounded-lg shadow-sm">
            <div class="flex items-start sm:items-center gap-2 sm:gap-3">
              <div class="bg-green-100 p-1.5 sm:p-2 rounded-full flex-shrink-0">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-green-600 w-[18px] h-[18px] sm:w-5 sm:h-5"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
              </div>
              <div>
                <p class="font-semibold text-green-800 text-sm sm:text-base">{$t('security.2fa.status.enabled')}</p>
                <p class="text-xs sm:text-sm text-green-700">{$t('security.2fa.status.enabled_desc')}</p>
              </div>
            </div>
            <Button variant="destructive" on:click={disable2FA} class="font-semibold text-sm w-full sm:w-auto">{$t('security.2fa.disable')}</Button>
          </div>
        {:else}
          <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 p-3 sm:p-4 bg-yellow-50 border-l-4 border-l-yellow-500 border-y border-r border-yellow-200 rounded-lg shadow-sm">
            <div class="flex items-start sm:items-center gap-2 sm:gap-3">
              <div class="bg-yellow-100 p-1.5 sm:p-2 rounded-full flex-shrink-0">
                <AlertCircle class="h-4 w-4 sm:h-5 sm:w-5 text-yellow-600" />
              </div>
              <div>
                <p class="font-semibold text-yellow-900 text-sm sm:text-base">Not Protected</p>
                <p class="text-xs sm:text-sm text-yellow-700">{$t('security.2fa.status.disabled_desc')}</p>
              </div>
            </div>
            <Button on:click={setup2FA} class="bg-blue-600 hover:bg-blue-700 text-white font-semibold text-sm w-full sm:w-auto">{$t('security.2fa.enable')}</Button>
          </div>
        {/if}
        <div class="border border-gray-200 dark:border-gray-700 rounded-lg p-3">
          <div class="flex items-start gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-blue-600 mt-0.5 flex-shrink-0"><circle cx="12" cy="12" r="10"></circle><path d="M12 16v-4"></path><path d="M12 8h.01"></path></svg>
        <p class="text-sm text-muted-foreground">{$t('security.2fa.how_it_works')}</p>
          </div>
        </div>
      </div>
  </Card>
  {/if}

  {#if $etcAccount}
  <Card class="p-6" id="keystore-section">
    <div class="flex items-center gap-3 mb-4">
      <div class="bg-amber-100 p-2 rounded-lg">
        <KeyRound class="h-5 w-5 text-amber-600" />
      </div>
      <div>
      <h2 class="text-lg font-semibold">{$t('keystore.title')}</h2>
        <p class="text-xs text-muted-foreground">{$t('keystore.desc')}</p>
      </div>
    </div>
    <div class="space-y-4">
      <div class="flex items-center gap-2">
        <div class="flex-1">
          <Input
            type="password"
            bind:value={keystorePassword}
            placeholder={$t('placeholders.password')}
            class="w-full {passwordStrength === 'strong' ? 'border-green-500 focus:ring-green-500' : passwordStrength === 'medium' ? 'border-yellow-500 focus:ring-yellow-500' : passwordStrength === 'weak' ? 'border-red-500 focus:ring-red-500' : ''}"
            autocomplete="new-password"
          />
          {#if keystorePassword}
            <!-- Enhanced Password Strength Indicator -->
            <div class="mt-2 space-y-2">
              <div class="flex items-center justify-between">
                <span class="text-xs font-medium text-gray-700">Password Strength</span>
                <span class="text-xs font-semibold {passwordStrength === 'strong' ? 'text-green-600' : passwordStrength === 'medium' ? 'text-yellow-600' : 'text-red-600'}">
                  {passwordFeedback}
                </span>
              </div>
              <div class="h-2 w-full bg-gray-200 rounded-full overflow-hidden shadow-inner">
                <div
                  class="h-full transition-all duration-500 ease-out {passwordStrength === 'strong' ? 'bg-gradient-to-r from-green-500 to-green-600 w-full' : passwordStrength === 'medium' ? 'bg-gradient-to-r from-yellow-500 to-yellow-600 w-2/3' : 'bg-gradient-to-r from-red-500 to-red-600 w-1/3'}"
                ></div>
              </div>
            </div>
            
            <!-- Enhanced Requirements Checklist with Icons -->
            <div class="mt-3 border border-gray-200 dark:border-gray-700 rounded-lg p-3 bg-gray-50 dark:bg-gray-900/30">
              <p class="text-xs font-semibold text-gray-700 dark:text-gray-300 mb-2">Requirements:</p>
              <ul class="space-y-1.5">
                <li class="flex items-center gap-2 text-xs transition-all duration-300 {keystorePassword.length >= 8 ? 'text-green-600 font-medium' : 'text-gray-500'}">
                  {#if keystorePassword.length >= 8}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
                  {:else}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><circle cx="12" cy="12" r="10"></circle></svg>
                  {/if}
                  <span>{$t('password.requirements.length')}</span>
                </li>
                <li class="flex items-center gap-2 text-xs transition-all duration-300 {/[A-Z]/.test(keystorePassword) ? 'text-green-600 font-medium' : 'text-gray-500'}">
                  {#if /[A-Z]/.test(keystorePassword)}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
                  {:else}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><circle cx="12" cy="12" r="10"></circle></svg>
                  {/if}
                  <span>{$t('password.requirements.uppercase')}</span>
                </li>
                <li class="flex items-center gap-2 text-xs transition-all duration-300 {/[a-z]/.test(keystorePassword) ? 'text-green-600 font-medium' : 'text-gray-500'}">
                  {#if /[a-z]/.test(keystorePassword)}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
                  {:else}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><circle cx="12" cy="12" r="10"></circle></svg>
                  {/if}
                  <span>{$t('password.requirements.lowercase')}</span>
                </li>
                <li class="flex items-center gap-2 text-xs transition-all duration-300 {/[0-9]/.test(keystorePassword) ? 'text-green-600 font-medium' : 'text-gray-500'}">
                  {#if /[0-9]/.test(keystorePassword)}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
                  {:else}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><circle cx="12" cy="12" r="10"></circle></svg>
                  {/if}
                  <span>{$t('password.requirements.number')}</span>
                </li>
                <li class="flex items-center gap-2 text-xs transition-all duration-300 {/[!@#$%^&*(),.?":{}|<>]/.test(keystorePassword) ? 'text-green-600 font-medium' : 'text-gray-500'}">
                  {#if /[!@#$%^&*(),.?":{}|<>]/.test(keystorePassword)}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
                  {:else}
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="flex-shrink-0"><circle cx="12" cy="12" r="10"></circle></svg>
                  {/if}
                  <span>{$t('password.requirements.special')}</span>
                </li>
            </ul>
            </div>
          {/if}
        </div>
        <Button
          on:click={saveToKeystore}
          disabled={!isPasswordValid || isSavingToKeystore}
          class="bg-green-600 hover:bg-green-700 text-white font-semibold"
        >
          {#if isSavingToKeystore}
            <div class="flex items-center gap-2">
              <div class="animate-spin rounded-full h-4 w-4 border-2 border-white border-t-transparent"></div>
            {$t('actions.saving')}
            </div>
          {:else}
            <div class="flex items-center gap-2">
              <KeyRound class="h-4 w-4" />
            {$t('actions.saveKey')}
            </div>
          {/if}
        </Button>
      </div>
      {#if keystoreSaveMessage}
        <div class="mt-3 p-3 rounded-lg border-l-4 {keystoreSaveMessage.toLowerCase().includes('success') ? 'bg-green-50 border-l-green-500 border-y border-r border-green-200' : 'bg-red-50 border-l-red-500 border-y border-r border-red-200'}">
          <p class="text-sm {keystoreSaveMessage.toLowerCase().includes('success') ? 'text-green-700' : 'text-red-700'} flex items-center gap-2">
            {#if keystoreSaveMessage.toLowerCase().includes('success')}
              <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
            {:else}
              <AlertCircle class="h-4 w-4" />
            {/if}
            {keystoreSaveMessage}
          </p>
        </div>
      {/if}
    </div>
  </Card>
  {/if}
  
  {#if $etcAccount}
  <Card class="p-4 sm:p-6">
    <div class="flex items-start sm:items-center justify-between mb-3 sm:mb-4 gap-2">
      <div class="flex items-start sm:items-center gap-2 sm:gap-3">
        <div class="bg-red-100 p-1.5 sm:p-2 rounded-lg flex-shrink-0">
          <BadgeX class="h-4 w-4 sm:h-5 sm:w-5 text-red-600" />
        </div>
      <div>
        <h2 class="text-base sm:text-lg font-semibold">{$t('blacklist.title')}</h2>
          <p class="text-xs sm:text-sm text-muted-foreground">{$t('blacklist.subtitle')}</p>
      </div>
      </div>
    </div>

    <div class="space-y-4 sm:space-y-6">
      <div class="border border-gray-200 dark:border-gray-700 rounded-lg p-3 sm:p-4">
        <div class="flex items-center gap-1.5 sm:gap-2 mb-2 sm:mb-3">
          <div class="bg-red-100 p-1 sm:p-1.5 rounded">
            <Plus class="h-3.5 w-3.5 sm:h-4 sm:w-4 text-red-600" />
          </div>
          <h3 class="text-sm sm:text-base font-semibold">{$t('blacklist.add.title')}</h3>
        </div>
        <div class="space-y-3 sm:space-y-4">
          <div>
            <Label for="blacklist-address">{$t('blacklist.add.address')}</Label>
            <Input
              id="blacklist-address"
              bind:value={newBlacklistEntry.chiral_address}
              placeholder={$t('blacklist.add.addressPlaceholder')}
              class="mt-2 font-mono text-sm {isBlacklistAddressValid ? 'border-green-500 focus:ring-green-500' : newBlacklistEntry.chiral_address && !isBlacklistAddressValid ? 'border-red-500 focus:ring-red-500' : ''}"
            />
            <div class="flex items-center justify-between mt-1">
              <span class="text-xs text-muted-foreground">
                {newBlacklistEntry.chiral_address.length}/42 {$t('transfer.recipient.characters')}
                {#if newBlacklistEntry.chiral_address.length <= 42}
                  ({42 - newBlacklistEntry.chiral_address.length} {$t('transfer.recipient.remaining')})
                {:else}
                  ({newBlacklistEntry.chiral_address.length - 42} {$t('transfer.recipient.over')})
                {/if}
              </span>
              {#if blacklistAddressWarning}
                <p class="text-xs text-red-500 font-medium">{blacklistAddressWarning}</p>
              {/if}
            </div>
          </div>
          
          <div>
            <Label for="blacklist-reason">{$t('blacklist.add.reason')}</Label>
            <div class="relative mt-2">
              <Input
                id="blacklist-reason"
                bind:value={newBlacklistEntry.reason}
                placeholder={$t('placeholders.reason')}
                maxlength={200}
                class="pr-16"
              />
              <span class="absolute right-3 top-1/2 transform -translate-y-1/2 text-xs text-muted-foreground">
                {newBlacklistEntry.reason.length}/200
              </span>
            </div>
            {#if newBlacklistEntry.reason.length > 150}
              <p class="text-xs text-orange-500 mt-1">
                {$t('blacklist.add.remaining', { values: { remaining: 200 - newBlacklistEntry.reason.length } })}
              </p>
            {/if}
          </div>

          <div>
            <Label class="text-xs mb-2 block font-semibold">{$t('blacklist.quickReasons.label')}</Label>
            <div class="flex flex-wrap gap-2">
            {#each [$t('blacklist.quickReasons.spam'), $t('blacklist.quickReasons.fraud'), $t('blacklist.quickReasons.malicious'), $t('blacklist.quickReasons.harassment'), $t('blacklist.quickReasons.scam')] as reason}
              <button
                type="button"
                  class="px-3 py-1.5 text-xs font-semibold border-2 rounded-full transition-all shadow-sm {newBlacklistEntry.reason === reason ? 'bg-red-600 text-white border-red-600 shadow-md scale-105' : 'bg-white border-gray-300 hover:border-red-400 hover:bg-red-50 hover:shadow-md hover:scale-105'}"
                on:click={() => newBlacklistEntry.reason = reason}
              >
                {reason}
              </button>
            {/each}
            </div>
          </div>

          <Button 
            type="button" 
            class="w-full bg-red-600 hover:bg-red-700 text-white font-semibold" 
            disabled={!isBlacklistFormValid} 
            on:click={addBlacklistEntry}
          >
            <div class="flex items-center gap-2">
              <BadgeX class="h-4 w-4" />
            {$t('blacklist.add.submit')}
            </div>
          </Button>
        </div>
      </div>

      <div>
        <div class="flex items-center justify-between mb-4">
          <div class="flex items-center gap-2">
            <h3 class="text-md font-semibold">{$t('blacklist.list.title')}</h3>
            {#if $blacklist.length > 0}
              <span class="bg-red-100 text-red-700 text-xs font-semibold px-2 py-1 rounded-full">{$blacklist.length}</span>
            {/if}
          </div>
          
          <div class="flex gap-2">
            <div class="relative">
              <svg class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-red-400 pointer-events-none" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"></path>
              </svg>
              <Input
                bind:value={blacklistSearch}
                placeholder={$t('placeholders.searchBlacklist')}
                class="w-96 text-sm pl-10 pr-10 border-gray-300 focus:border-red-400 focus:ring-red-400"
              />
              {#if blacklistSearch}
                <button
                  type="button"
                  class="absolute right-2 top-1/2 -translate-y-1/2 text-red-400 hover:text-red-600 p-1 rounded-full hover:bg-red-50 transition-colors"
                  on:click={clearBlacklistSearch}
                  title={$t('tooltips.clearSearch')}
                  aria-label={$t('transactions.clearSearch')}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
                </button>
              {/if}
            </div>
            
            {#if $blacklist.length > 0}
              <Button 
                size="sm" 
                variant="outline" 
                on:click={clearAllBlacklist}
                class="bg-red-50 text-red-700 border-red-300 hover:bg-red-100 hover:border-red-400 font-semibold shadow-sm"
              >
                <div class="flex items-center gap-1.5">
                  <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="m19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path><line x1="10" y1="11" x2="10" y2="17"></line><line x1="14" y1="11" x2="14" y2="17"></line></svg>
                {$t('blacklist.actions.clearAll')}
                </div>
              </Button>
            {/if}
          </div>
        </div>

        {#if filteredBlacklist.length === 0 && $blacklist.length === 0}
          <div class="text-center py-8 border-2 border-dashed border-gray-300 rounded-lg bg-gray-50">
            <div class="bg-gray-200 rounded-full p-3 w-16 h-16 mx-auto mb-3 flex items-center justify-center">
              <BadgeX class="h-8 w-8 text-gray-400" />
            </div>
            <p class="font-semibold text-gray-700">{$t('blacklist.list.emptyTitle')}</p>
            <p class="text-sm mt-1 text-gray-500">{$t('blacklist.list.emptyDesc')}</p>
          </div>
        {:else if filteredBlacklist.length === 0 && blacklistSearch}
          <div class="text-center py-6 bg-yellow-50 border border-yellow-200 rounded-lg">
            <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-yellow-600 mx-auto mb-2"><circle cx="12" cy="12" r="10"></circle><path d="m16 16-4-4-4 4"></path><path d="m8 8 4 4 4-4"></path></svg>
            <p class="text-yellow-800 font-medium">{$t('blacklist.list.noMatch', { values: { q: blacklistSearch } })}</p>
          </div>
        {:else}
          <div class="space-y-2 max-h-64 overflow-y-auto">
            {#each filteredBlacklist as entry, index (entry.chiral_address)}
              <div class="flex items-start justify-between p-4 bg-red-50 border-l-4 border-l-red-500 border-y border-r border-red-200 rounded-lg group hover:shadow-md transition-all">
                <div class="flex items-start gap-3 flex-1 min-w-0">
                  <div class="bg-red-100 p-2 rounded-full flex-shrink-0 mt-0.5">
                    <BadgeX class="h-4 w-4 text-red-600" />
                  </div>
                  
                <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2 mb-2">
                      <p class="text-sm font-mono font-semibold text-red-900 truncate">
                      {entry.chiral_address}
                    </p>
                    <button
                      type="button"
                        class="opacity-0 group-hover:opacity-100 transition-all p-1.5 bg-red-100 hover:bg-red-200 border border-red-300 hover:border-red-400 rounded flex-shrink-0 shadow-sm hover:shadow-md"
                      on:click={() => copyToClipboard(entry.chiral_address)}
                      title={$t('blacklist.actions.copyAddress')}
                        aria-label={$t('blacklist.actions.copyAddress')}
                    >
                        <Copy class="h-3.5 w-3.5 text-red-700" />
                    </button>
                  </div>

                  {#if editingEntry === index}
                      <div class="space-y-2 bg-white p-3 rounded-lg border-2 border-blue-300 shadow-md">
                      <Input
                        bind:value={editReason}
                        placeholder={$t('placeholders.reason')}
                        maxlength={200}
                          class="text-sm border-blue-300 focus:border-blue-500 focus:ring-blue-500"
                        on:keydown={handleEditKeydown}
                        autofocus
                      />
                      <div class="flex gap-2">
                          <Button size="sm" on:click={saveEdit} disabled={!editReason.trim()} class="bg-green-600 hover:bg-green-700 text-white font-semibold">
                            <div class="flex items-center gap-1">
                              <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
                          {$t('actions.save')}
                            </div>
                        </Button>
                          <Button size="sm" variant="outline" on:click={cancelEdit} class="hover:bg-gray-100">
                          {$t('actions.cancel')}
                        </Button>
                      </div>
                    </div>
                  {:else}
                      <div class="bg-red-100 border border-red-200 rounded-md px-3 py-2 mb-2">
                        <p class="text-xs font-medium text-red-800"><span class="font-semibold">Reason:</span> {entry.reason}</p>
                      </div>
                      <div class="flex items-center gap-1 text-xs text-red-700">
                        <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><polyline points="12 6 12 12 16 14"></polyline></svg>
                    {$t('blacklist.list.addedAt', { values: { date: formatDate(entry.timestamp) } })}
                      </div>
                    {/if}
                  </div>
                </div>
                
                <div class="flex items-center gap-2 ml-4 flex-shrink-0">
                  {#if editingEntry !== index}
                    <Button 
                      size="sm" 
                      variant="outline"
                      on:click={() => startEditEntry(index)}
                      class="opacity-0 group-hover:opacity-100 transition-all bg-blue-50 text-blue-700 border-blue-300 hover:bg-blue-100 hover:border-blue-400 font-medium"
                    >
                      <div class="flex items-center gap-1">
                        <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"></path><path d="m15 5 4 4"></path></svg>
                      {$t('actions.edit')}
                      </div>
                    </Button>
                  {/if}
                  
                  <Button 
                    size="sm" 
                    variant="destructive"
                    on:click={() => removeBlacklistEntry(entry.chiral_address)}
                    disabled={editingEntry === index}
                    class="bg-red-600 hover:bg-red-700 text-white font-semibold shadow-sm"
                  >
                    <div class="flex items-center gap-1">
                      <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
                    {$t('actions.remove')}
                    </div>
                  </Button>
                </div>
              </div>
            {/each}
          </div>
          
          {#if $blacklist.length > 5}
            <div class="text-center mt-3 p-2 bg-gray-100 rounded-lg border border-gray-200">
              <p class="text-xs text-gray-700 font-medium">
                {$t('blacklist.list.showing', { values: { shown: filteredBlacklist.length, total: $blacklist.length } })}
              </p>
            </div>
          {/if}
        {/if}
      </div>

        <div class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-4">
          <div class="grid grid-cols-2 gap-3">
            {#if $blacklist.length > 0}
            <Button 
              variant="outline" 
              size="sm" 
              on:click={exportBlacklist}
              class="bg-green-50 text-green-700 border-green-300 hover:bg-green-100 hover:border-green-400 font-semibold shadow-sm"
              disabled={$blacklist.length === 0}
              title={$t('blacklist.actions.exportTitle', { values: { count: $blacklist.length } })}
            >
              <div class="flex items-center justify-center gap-2">
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path><polyline points="7 10 12 15 17 10"></polyline><line x1="12" y1="15" x2="12" y2="3"></line></svg>
                {$t('blacklist.actions.export')} ({$blacklist.length})
              </div>
            </Button>
            {/if}
            <Button 
              variant="outline" 
              size="sm" 
              on:click={() => importFileInput.click()}
              class="{$blacklist.length === 0 ? 'col-span-2' : ''} bg-blue-50 text-blue-700 border-blue-300 hover:bg-blue-100 hover:border-blue-400 font-semibold shadow-sm"
            >
              <div class="flex items-center justify-center gap-2">
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path><polyline points="17 8 12 3 7 8"></polyline><line x1="12" y1="3" x2="12" y2="15"></line></svg>
              {$t('blacklist.actions.import')}
              </div>
            </Button>
          </div>
          
          <input
            bind:this={importFileInput}
            type="file"
            accept=".json"
            class="hidden"
            on:change={handleImportFile}
          />
        </div>
      
    </div>
  </Card>
  {/if}

  <!-- Transaction Receipt Modal -->
  <TransactionReceipt
    transaction={selectedTransaction}
    isOpen={showTransactionReceipt}
    onClose={closeTransactionReceipt}
  />

  <!-- 2FA Setup Modal -->
  {#if show2faSetupModal && totpSetupInfo}
    <div
      class="fixed inset-0 bg-black/50 backdrop-blur-md flex items-center justify-center z-50 p-3 sm:p-4 animate-in fade-in duration-200"
      role="button"
      tabindex="0"
      on:click={() => show2faSetupModal = false}
      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') show2faSetupModal = false; }}
    >
      <div
        class="bg-white dark:bg-gray-900 p-5 sm:p-7 rounded-2xl shadow-[0_20px_60px_rgba(0,0,0,0.3)] w-full max-w-lg border border-blue-200 dark:border-blue-800 animate-in zoom-in-95 duration-200 max-h-[90vh] overflow-y-auto"
        on:click|stopPropagation
        role="dialog"
        aria-modal="true"
        tabindex="-1"
        on:keydown={(e) => { if (e.key === 'Escape') show2faSetupModal = false; }}
      >
        <!-- Header with Icon -->
        <div class="flex items-center justify-between mb-6">
          <div class="flex items-center gap-3">
            <div class="bg-blue-100 p-2.5 rounded-xl shadow-sm">
              <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-blue-600"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>
            </div>
            <div>
              <h3 class="text-2xl font-bold text-gray-900 dark:text-white">{$t('security.2fa.setup.title')}</h3>
              <p class="text-xs text-blue-600 font-semibold mt-0.5">Step 1 of 2</p>
            </div>
          </div>
          <button
            on:click={() => show2faSetupModal = false}
            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800"
            aria-label="Close"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
          </button>
        </div>
        
        <!-- Step 1 Info -->
        <div class="bg-blue-50 border-l-4 border-l-blue-500 p-4 rounded-xl mb-5 shadow-sm">
          <p class="text-sm text-blue-900 font-medium">{$t('security.2fa.setup.step1_scan')}</p>
        </div>
        
        <!-- QR Code & Secret -->
        <div class="flex flex-col md:flex-row gap-5 items-center bg-gradient-to-br from-gray-50 to-blue-50 dark:from-gray-800 dark:to-blue-900/20 border border-gray-200 dark:border-gray-700 p-5 rounded-xl mb-5 shadow-sm">
          <div class="bg-white p-4 rounded-xl border-2 border-blue-300 shadow-lg transition-transform hover:scale-105 duration-200">
            <img src={totpSetupInfo.qrCodeDataUrl} alt="2FA QR Code" class="w-40 h-40 rounded-lg" />
          </div>
          <div class="space-y-3 flex-1">
            <p class="text-sm font-semibold text-gray-900 dark:text-white">{$t('security.2fa.setup.scanAlt')}</p>
            <p class="text-xs text-gray-600 dark:text-gray-400">{$t('security.2fa.setup.step2_manual')}</p>
            <div class="flex items-center gap-2 bg-white dark:bg-gray-800 p-3 rounded-lg border-2 border-blue-200 dark:border-blue-700 shadow-sm">
              <code class="text-sm font-mono break-all text-blue-700 dark:text-blue-300 flex-1 font-semibold">{totpSetupInfo.secret}</code>
              <Button 
                size="icon" 
                variant="ghost" 
                on:click={() => { navigator.clipboard.writeText(totpSetupInfo?.secret || ''); showToast(tr('toasts.common.copied'), 'success'); }} 
                class="flex-shrink-0 hover:bg-blue-100 hover:text-blue-700 transition-colors"
              >
                <Copy class="h-4 w-4" />
              </Button>
            </div>
          </div>
        </div>

        <!-- Step 2 Info -->
        <div class="bg-gradient-to-r from-purple-50 to-pink-50 dark:from-purple-900/20 dark:to-pink-900/20 border-l-4 border-l-purple-500 p-4 rounded-xl mb-5 shadow-sm">
          <p class="text-xs text-purple-900 dark:text-purple-200 font-semibold mb-1">Step 2 of 2</p>
          <p class="text-sm text-purple-800 dark:text-purple-300 font-medium">{$t('security.2fa.setup.step3_verify')}</p>
        </div>
        
        <!-- Input Fields -->
        <div class="space-y-4">
          <div>
            <Label for="totp-verify" class="text-sm font-semibold text-gray-700 dark:text-gray-300">{$t('security.2fa.setup.verifyLabel')}</Label>
          <Input
            id="totp-verify"
            type="text"
            bind:value={totpVerificationCode}
            placeholder="123456"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength={6}
              class="text-center text-2xl font-mono tracking-widest mt-2 h-14 border-2 border-blue-200 focus:border-blue-500 shadow-sm rounded-xl"
          />
          </div>
          <div>
            <Label for="totp-password-setup" class="text-sm font-semibold text-gray-700 dark:text-gray-300">{$t('keystore.load.password')}</Label>
          <Input
            id="totp-password-setup"
            type="password"
            bind:value={twoFaPassword}
            placeholder={$t('placeholders.unlockPassword')}
              class="mt-2 h-11 border-2 border-gray-200 focus:border-blue-500 shadow-sm rounded-xl"
          />
          </div>
          {#if twoFaErrorMessage}
            <div class="bg-red-50 border-l-4 border-l-red-500 p-4 rounded-xl shadow-sm animate-in slide-in-from-top-2 duration-200">
              <p class="text-sm text-red-700 dark:text-red-600 flex items-center gap-2 font-medium">
                <AlertCircle class="h-5 w-5 flex-shrink-0" />
                {twoFaErrorMessage}
              </p>
            </div>
          {/if}
        </div>

        <!-- Action Buttons -->
        <div class="mt-7 flex justify-end gap-3">
          <Button 
            variant="outline" 
            on:click={() => show2faSetupModal = false}
            class="px-6 font-medium hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
          >
            {$t('actions.cancel')}
          </Button>
          <Button 
            on:click={verifyAndEnable2FA} 
            disabled={isVerifying2fa || totpVerificationCode.length < 6 || !twoFaPassword} 
            class="bg-green-600 hover:bg-green-700 text-white font-semibold px-6 shadow-md hover:shadow-lg transition-all duration-200"
          >
            {#if isVerifying2fa}
              <div class="flex items-center gap-2">
                <div class="animate-spin rounded-full h-4 w-4 border-2 border-white border-t-transparent"></div>
                {$t('actions.verifying')}
        </div>
            {:else}
              <div class="flex items-center gap-2">
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>
                {$t('security.2fa.setup.verifyAndEnable')}
              </div>
            {/if}
          </Button>
        </div>
      </div>
    </div>
  {/if}

  <!-- 2FA Action Prompt Modal -->
  {#if show2faPromptModal}
    <div
      class="fixed inset-0 bg-black/50 backdrop-blur-md flex items-center justify-center z-50 p-3 sm:p-4 animate-in fade-in duration-200"
      role="button"
      tabindex="0"
      on:click={() => { show2faPromptModal = false; actionToConfirm = null; }}
      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { show2faPromptModal = false; actionToConfirm = null; } }}
    >
      <div
        class="bg-white dark:bg-gray-900 p-5 sm:p-7 rounded-2xl shadow-[0_20px_60px_rgba(0,0,0,0.3)] w-full max-w-md border border-blue-200 dark:border-blue-800 animate-in zoom-in-95 duration-200 max-h-[90vh] overflow-y-auto"
        on:click|stopPropagation
        role="dialog"
        aria-modal="true"
        tabindex="-1"
        on:keydown={(e) => { if (e.key === 'Escape' ) { show2faPromptModal = false; actionToConfirm = null; } }}
      >
        <!-- Header -->
        <div class="flex items-center justify-between mb-6">
          <div class="flex items-center gap-3">
            <div class="bg-blue-100 p-2.5 rounded-xl shadow-sm">
              <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-blue-600"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>
            </div>
            <div>
              <h3 class="text-2xl font-bold text-gray-900 dark:text-white">{$t('security.2fa.prompt.title')}</h3>
              <p class="text-sm text-gray-600 dark:text-gray-400 mt-0.5">{$t('security.2fa.prompt.enter_code')}</p>
            </div>
          </div>
          <button
            on:click={() => { show2faPromptModal = false; actionToConfirm = null; }}
            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800"
            aria-label="Close"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
          </button>
        </div>
        
        <!-- Input Fields -->
        <div class="space-y-4">
          <div>
            <Label for="totp-action" class="text-sm font-semibold text-gray-700 dark:text-gray-300">{$t('security.2fa.prompt.label')}</Label>
          <Input
            id="totp-action"
            type="text"
            bind:value={totpActionCode}
            placeholder="123456"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength={6}
            autofocus
              class="text-center text-2xl font-mono tracking-widest mt-2 h-14 border-2 border-blue-200 focus:border-blue-500 shadow-sm rounded-xl"
          />
          </div>
          <div>
            <Label for="totp-password-action" class="text-sm font-semibold text-gray-700 dark:text-gray-300">{$t('keystore.load.password')}</Label>
          <Input
            id="totp-password-action"
            type="password"
            bind:value={twoFaPassword}
            placeholder={$t('placeholders.unlockPassword')}
              class="mt-2 h-11 border-2 border-gray-200 focus:border-blue-500 shadow-sm rounded-xl"
          />
          </div>
          {#if twoFaErrorMessage}
            <div class="bg-red-50 border-l-4 border-l-red-500 p-4 rounded-xl shadow-sm animate-in slide-in-from-top-2 duration-200">
              <p class="text-sm text-red-700 dark:text-red-600 flex items-center gap-2 font-medium">
                <AlertCircle class="h-5 w-5 flex-shrink-0" />
                {twoFaErrorMessage}
              </p>
            </div>
          {/if}
        </div>

        <!-- Action Buttons -->
        <div class="mt-7 flex justify-end gap-3">
          <Button 
            variant="outline" 
            on:click={() => { show2faPromptModal = false; actionToConfirm = null; }}
            class="px-6 font-medium hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
          >
            {$t('actions.cancel')}
          </Button>
          <Button 
            on:click={confirmActionWith2FA} 
            disabled={isVerifyingAction || totpActionCode.length < 6 || !twoFaPassword} 
            class="bg-blue-600 hover:bg-blue-700 text-white font-semibold px-6 shadow-md hover:shadow-lg transition-all duration-200"
          >
            {#if isVerifyingAction}
              <div class="flex items-center gap-2">
                <div class="animate-spin rounded-full h-4 w-4 border-2 border-white border-t-transparent"></div>
                {$t('actions.verifying')}
        </div>
            {:else}
              <div class="flex items-center gap-2">
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>
                {$t('actions.confirm')}
              </div>
            {/if}
          </Button>
        </div>
      </div>
  </div>
  {/if}
</div>
