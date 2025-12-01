<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Wallet, Copy, ArrowUpRight, ArrowDownLeft, History, Coins, Plus, Import, BadgeX, KeyRound, FileText, AlertCircle, RefreshCw } from 'lucide-svelte'
  import DropDown from "$lib/components/ui/dropDown.svelte";
  import { wallet, etcAccount, blacklist, settings } from '$lib/stores'
  import { gethStatus } from '$lib/services/gethService'
  import { walletService } from '$lib/wallet';
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
  import { totalSpent, totalReceived, miningState, accurateTotals, isCalculatingAccurateTotals, accurateTotalsProgress } from '$lib/stores';
  import { goto } from '@mateothegreat/svelte5-router';

  const tr = (k: string, params?: Record<string, any>): string => $t(k, params)
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
  let selectedTransaction: any = null;
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
  
  // Fee preset (UI stub only)
  let feePreset: 'low' | 'market' | 'fast' = 'market'
  let estimatedFeeDisplay: string = '—'
  let estimatedFeeNumeric: number = 0
  
  // Confirmation for sending transaction
  let isConfirming = false
  let countdown = 0
  let intervalId: number | null = null

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

          // 'transactions' shows sent + received (excludes mining)
          const matchesType = filterType === 'transactions'
            ? (tx.type === 'sent' || tx.type === 'received')
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

          return matchesType && fromOk && toOk && matchesSearch;
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

  // Amount validation
  $: {
    if (rawAmountInput === '') {
      validationWarning = '';
      isAmountValid = false;
      sendAmount = 0;
    } else {
      const inputValue = parseFloat(rawAmountInput);

      if (isNaN(inputValue) || inputValue <= 0) {
        validationWarning = tr('errors.amount.invalid');
        isAmountValid = false;
        sendAmount = 0;
      } else if (inputValue < 0.01) {
        validationWarning = tr('errors.amount.min', { min: '0.01' });
        isAmountValid = false;
        sendAmount = 0;
      } else if (inputValue > $wallet.balance) {
        validationWarning = tr('errors.amount.insufficient', { values: { more: (inputValue - $wallet.balance).toFixed(2) } });
        isAmountValid = false;
        sendAmount = 0;
      } else if (inputValue + estimatedFeeNumeric > $wallet.balance) {
        validationWarning = tr('errors.amount.insufficientWithFee', {
          values: {
            total: (inputValue + estimatedFeeNumeric).toFixed(2),
            balance: $wallet.balance.toFixed(2)
          }
        });
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

  // Mock estimated fee calculation (UI-only) - separate from validation
  $: estimatedFeeNumeric = rawAmountInput && parseFloat(rawAmountInput) > 0 ? parseFloat((parseFloat(rawAmountInput) * { low: 0.0025, market: 0.005, fast: 0.01 }[feePreset]).toFixed(4)) : 0
  $: estimatedFeeDisplay = rawAmountInput && parseFloat(rawAmountInput) > 0 ? `${estimatedFeeNumeric.toFixed(4)} Chiral` : '—'

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
    
    
    // showToast('Address copied to clipboard!', 'success')
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
          // showToast('Failed to retrieve private key', 'error');
          showToast(tr('toasts.account.privateKey.fetchError'), 'error');
          return;
        }
      }
      
      if (privateKeyToCopy) {
        navigator.clipboard.writeText(privateKeyToCopy);
        // showToast('Private key copied to clipboard!', 'success');
        showToast(tr('toasts.account.privateKey.copied'), 'success');
      }
      else {
        // showToast('No private key available', 'error');
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
        
        // Check if the File System Access API is supported
        if ('showSaveFilePicker' in window) {
          try {
            const fileHandle = await (window as any).showSaveFilePicker({
              suggestedName: `chiral-wallet-export-${new Date().toISOString().split('T')[0]}.json`,
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
          } catch (error: any) {
            if (error.name !== 'AbortError') {
              throw error;
            }
            // User cancelled, don't show error message
            return;
          }
        } else {
          // Fallback for browsers that don't support File System Access API
          const url = URL.createObjectURL(dataBlob);
          const link = document.createElement('a');
          link.href = url;
          link.download = `chiral-wallet-export-${new Date().toISOString().split('T')[0]}.json`;
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
    // showToast('Transaction cancelled', 'warning')
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
      
      // showToast('Transaction submitted!', 'success')
      showToast(tr('toasts.account.transaction.submitted'), 'success')
      
    } catch (error) {
      console.error('Transaction failed:', error)
      // showToast('Transaction failed: ' + String(error), 'error')
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

  function handleTransactionClick(tx: any) {
    selectedTransaction = tx;
    showTransactionReceipt = true;
  }

  function closeTransactionReceipt() {
    showTransactionReceipt = false;
    selectedTransaction = null;
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
    if (!isTauri || !isGethRunning || !$etcAccount) return
    try {
      await walletService.refreshBalance()
    } catch (error) {
      console.error('Failed to fetch balance:', error)
    }
  }

  async function calculateAccurateTotals() {
    try {
      await walletService.calculateAccurateTotals();
      console.log('Accurate totals calculated successfully');
    } catch (error) {
      console.error('Failed to calculate accurate totals:', error);
    }
  }

  // Automatically calculate accurate totals when account is loaded
  $: if ($etcAccount && isGethRunning && !$accurateTotals && !$isCalculatingAccurateTotals) {
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

    // showToast('Account Created Successfully!', 'success')
    showToast(tr('toasts.account.created'), 'success')
    
    if (isGethRunning) {
      await walletService.refreshBalance()
    }
  } catch (error) {
    console.error('Failed to create Chiral account:', error)
    // showToast('Failed to create account: ' + String(error), 'error')
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
    function onScanSuccess(decodedText: string, _decodedResult: any) {
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
      // showToast(validation.error || 'Invalid private key format', 'error')
      showToast(validation.error || tr('toasts.account.import.invalidFormat'), 'error')
      return
    }

    isImportingAccount = true
    try {
      const account = await walletService.importAccount(importPrivateKey)
      wallet.update(w => ({
        ...w,
        address: account.address,

        pendingTransactions: 0
      }))
      importPrivateKey = ''


      // showToast('Account imported successfully!', 'success')
      showToast(tr('toasts.account.import.success'), 'success')

      if (isGethRunning) {
        await walletService.refreshBalance()
      }
    } catch (error) {
      console.error('Failed to import Chiral account:', error)


      // showToast('Failed to import account: ' + String(error), 'error')
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
          if (!accountData.privateKey) {
            // showToast('Invalid file format: privateKey field not found', 'error');
            showToast(tr('toasts.account.import.fileInvalid'), 'error');
            return;
          }
          
          // Extract and set the private key
          importPrivateKey = accountData.privateKey;
          // showToast('Private key loaded from file successfully!', 'success');
          showToast(tr('toasts.account.import.fileSuccess'), 'success');
          
        } catch (error) {
          console.error('Error reading file:', error);
          // showToast('Error reading file: ' + String(error), 'error');
          showToast(
            tr('toasts.account.import.fileReadError', { values: { error: String(error) } }),
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
      // showToast('Error loading file: ' + String(error), 'error');
      showToast(
        tr('toasts.account.import.fileLoadError', { values: { error: String(error) } }),
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

            wallet.update(w => ({
                ...w,
                address: account.address
            }));

            // Clear sensitive data
            loadKeystorePassword = '';

            if (isGethRunning) {
                await walletService.refreshBalance();
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
      // showToast('2FA is only available in the desktop app.', 'warning');
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
      // showToast('Failed to start 2FA setup: ' + String(err), 'error');
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
        // showToast('Two-Factor Authentication has been enabled!', 'success');
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
        // showToast('Two-Factor Authentication has been disabled.', 'warning');
        showToast(tr('toasts.account.2fa.disabled'), 'warning');
      } catch (error) {
        console.error('Failed to disable 2FA:', error);
        // showToast('Failed to disable 2FA: ' + String(error), 'error');
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
  function setMaxAmount() {
    rawAmountInput = $wallet.balance.toFixed(2);
  }

  // async function handleLogout() {
  //   if (isTauri) await invoke('logout');
  //   logout();
  // }
  
  // Update your handleLogout function
  async function handleLogout() {
    try {
      // Stop mining if it's currently running
      if ($miningState.isMining) {
        await invoke('stop_miner');
      }
      
      // Call backend logout to clear active account from app state
      if (isTauri) {
        await invoke('logout');
      }
      
      // Clear the account store
      etcAccount.set(null);

      // Clear wallet data - reset to 0 balance, not a default value
      wallet.update((w: any) => ({
        ...w,
        address: "",
        balance: 0, // Reset to 0 for logout
        totalEarned: 0,
        totalSpent: 0,
        totalReceived: 0,
        pendingTransactions: 0
      }));

      // Clear mining state completely
      miningState.update((state: any) => ({
        ...state,
        isMining: false,
        hashRate: "0 H/s",
        totalRewards: 0,
        blocksFound: 0,
        activeThreads: 0,
        recentBlocks: [],
        sessionStartTime: undefined
      }));

      // Clear accurate totals (will recalculate on next login)
      accurateTotals.set(null);

      // Clear transaction history
      transactions.set([]);

      // Reset pagination states
      transactionPagination.set({
        accountAddress: null,
        oldestBlockScanned: null,
        isLoading: false,
        hasMore: true,
        batchSize: 5000,
      });
      miningPagination.set({
        accountAddress: null,
        oldestBlockScanned: null,
        isLoading: false,
        hasMore: true,
        batchSize: 5000,
      });

      // Clear any stored session data from both localStorage and sessionStorage
      if (typeof localStorage !== 'undefined') {
        localStorage.removeItem('lastAccount');
        localStorage.removeItem('miningSession');
        // Clear all sessionStorage data for security
        sessionStorage.clear();
      }
      
      privateKeyVisible = false;
      
      // Show success message
      // showToast('Wallet locked and session cleared', 'success');
      showToast(tr('toasts.account.logout.locked'), 'success');
      
    } catch (error) {
      console.error('Error during logout:', error);
      // showToast('Error during logout: ' + String(error), 'error');
      showToast(
        tr('toasts.account.logout.error', { values: { error: String(error) } }),
        'error'
      );
    }
  }

  async function generateAndShowQrCode(){
    const address = $etcAccount?.address;
    if(!address) return;
    try{
      qrCodeDataUrl = await QRCode.toDataURL(address, {
        errorCorrectionLevel: 'H',
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

  let sessionTimeout = 3600; // seconds (1 hour)
  let sessionTimer: number | null = null;
  let sessionCleanup: (() => void) | null = null;
  let autoLockMessage = '';

  function clearSessionTimer() {
    if (sessionTimer) {
      clearTimeout(sessionTimer);
      sessionTimer = null;
    }
  }

  function resetSessionTimer() {
    if (typeof window === 'undefined' || !$settings.enableWalletAutoLock) {
      clearSessionTimer();
      return;
    }
    clearSessionTimer();
    sessionTimer = window.setTimeout(() => {
      autoLockWallet();
    }, sessionTimeout * 1000);
  }

  function autoLockWallet() {
    if (!$settings.enableWalletAutoLock) return;
    handleLogout();
    autoLockMessage = 'Wallet auto-locked due to inactivity.';
    showToast(autoLockMessage, 'warning');
    setTimeout(() => autoLockMessage = '', 5000);
  }

  // Listen for user activity to reset timer
  function setupSessionTimeout() {
    if (typeof window === 'undefined') {
      return () => {};
    }
    const events = ['mousemove', 'keydown', 'mousedown', 'touchstart'];
    const handler = () => resetSessionTimer();
    for (const ev of events) {
      window.addEventListener(ev, handler);
    }
    resetSessionTimer();
    return () => {
      for (const ev of events) {
        window.removeEventListener(ev, handler);
      }
      clearSessionTimer();
    };
  }

  function teardownSessionTimeout() {
    if (sessionCleanup) {
      sessionCleanup();
      sessionCleanup = null;
    } else {
      clearSessionTimer();
    }
  }

  $: if (typeof window !== 'undefined') {
    if ($settings.enableWalletAutoLock) {
      if (!sessionCleanup) {
        sessionCleanup = setupSessionTimeout();
      } else {
        resetSessionTimer();
      }
    } else {
      teardownSessionTimeout();
    }
  }

  onMount(() => {
    if ($settings.enableWalletAutoLock && !sessionCleanup) {
      sessionCleanup = setupSessionTimeout();
    }
    return () => teardownSessionTimeout();
  });

</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('account.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('account.subtitle')}</p>
  </div>

  <!-- Warning Banner: Geth Not Running -->
  {#if $gethStatus !== 'running'}
    <div class="bg-yellow-500/10 border border-yellow-500/20 rounded-lg p-4">
      <div class="flex items-center gap-3">
        <AlertCircle class="h-5 w-5 text-yellow-500 flex-shrink-0" />
        <p class="text-sm text-yellow-600">
          {$t('nav.blockchainUnavailable')} <button on:click={() => { navigation.setCurrentPage('network'); goto('/network'); }} class="underline font-medium">{$t('nav.networkPageLink')}</button>. {$t('account.balanceWarning')}
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

  <div class="grid grid-cols-1 {$etcAccount ? 'md:grid-cols-2' : ''} gap-4">
    <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">{$t('wallet.title')}</h2>
        <Wallet class="h-5 w-5 text-muted-foreground" />
      </div>
      
      <div class="space-y-4">
        {#if !$etcAccount}
          <div class="space-y-3">
            <p class="text-sm text-muted-foreground">{$t('wallet.cta.intro')}</p>
            
            <Button 
              class="w-full" 
              on:click={createChiralAccount}
              disabled={isCreatingAccount}
            >
              <Plus class="h-4 w-4 mr-2" />
              {isCreatingAccount ? $t('actions.creating') : $t('actions.createAccount')}
            </Button>
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
              <Button variant="outline" class="w-full" on:click={openCreateMnemonic}>
                <KeyRound class="h-4 w-4 mr-2" /> {$t('wallet.hd.create_via_phrase')}
              </Button>
              <Button variant="outline" class="w-full" on:click={openImportMnemonic}>
                <Import class="h-4 w-4 mr-2" /> {$t('wallet.hd.import_phrase')}
              </Button>
            </div>
            
            <div class="space-y-2">
              <div class="flex w-full">
                <Input
                  type="text"
                  bind:value={importPrivateKey}
                  placeholder={$t('placeholders.importPrivateKey')}
                  class="flex-1 rounded-r-none border-r-0"
                  autocomplete="off"
                  data-form-type="other"
                  data-lpignore="true"
                  spellcheck="false"
                />
                <Button 
                  variant="outline"
                  size="default"
                  on:click={loadPrivateKeyFromFile}
                  class="rounded-l-none border-l-0 bg-gray-200 hover:bg-gray-300 border-gray-300 text-gray-900 shadow-sm"
                  title="Import private key from wallet JSON"
                >
                  <FileText class="h-4 w-4 mr-2" />
                  {$t('wallet.hd.load_from_wallet')}
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
                <span class="bg-card px-2 text-muted-foreground">{$t('wallet.cta.or')}</span>
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
          <p class="text-sm text-muted-foreground">{$t('wallet.balance')}</p>
          <p class="text-3xl font-bold text-foreground">{$wallet.balance.toFixed(8)} Chiral</p>
        </div>
        
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-4 mt-4">
          <!-- Blocks Mined -->
          <div class="min-w-0 border border-gray-200 dark:border-gray-700 rounded-md p-3 shadow-sm">
            <div class="flex items-center gap-2 mb-2">
              <div class="bg-purple-100 rounded p-1">
                <Coins class="h-4 w-4 text-purple-600" />
              </div>
            <p class="text-xs text-muted-foreground truncate">Blocks Mined {#if !$accurateTotals}<span class="text-xs opacity-60">(est.)</span>{/if}</p>
            </div>
            {#if $accurateTotals}
              <p class="text-base font-semibold text-foreground break-words">{$accurateTotals.blocksMined.toLocaleString()} blocks</p>
            {:else}
              <p class="text-base font-semibold text-foreground opacity-60 break-words">{$miningState.blocksFound.toLocaleString()} blocks</p>
            {/if}
          </div>
          <!-- Total Received -->
          <div class="min-w-0 border border-gray-200 dark:border-gray-700 rounded-md p-3 shadow-sm">
            <div class="flex items-center gap-2 mb-2">
              <div class="bg-green-100 rounded p-1">
                <ArrowDownLeft class="h-4 w-4 text-green-600" />
              </div>
            <p class="text-xs text-muted-foreground truncate">{$t('wallet.totalReceived')} {#if !$accurateTotals}<span class="text-xs opacity-60">(est.)</span>{/if}</p>
            </div>
            {#if $accurateTotals}
              <p class="text-base font-semibold text-green-600 dark:text-green-400 break-words">+{$accurateTotals.totalReceived.toFixed(8)}</p>
            {:else}
              <p class="text-base font-semibold text-green-600 dark:text-green-400 opacity-60 break-words">+{$totalReceived.toFixed(8)}</p>
            {/if}
          </div>
          <!-- Total Spent -->
          <div class="min-w-0 border border-gray-200 dark:border-gray-700 rounded-md p-3 shadow-sm">
            <div class="flex items-center gap-2 mb-2">
              <div class="bg-red-100 rounded p-1">
                <ArrowUpRight class="h-4 w-4 text-red-600" />
              </div>
            <p class="text-xs text-muted-foreground truncate">{$t('wallet.totalSpent')} {#if !$accurateTotals}<span class="text-xs opacity-60">(est.)</span>{/if}</p>
            </div>
            {#if $accurateTotals}
              <p class="text-base font-semibold text-red-600 dark:text-red-400 break-words">-{$accurateTotals.totalSent.toFixed(8)}</p>
            {:else}
              <p class="text-base font-semibold text-red-600 dark:text-red-400 opacity-60 break-words">-{$totalSpent.toFixed(8)}</p>
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

            <div class="mt-6">
              <p class="text-sm text-muted-foreground">{$t('wallet.address')}</p>
              <div class="flex items-center gap-2 mt-1">
                <p class="font-mono text-sm">{$etcAccount.address.slice(0, 10)}...{$etcAccount.address.slice(-8)}</p>
                <Button size="sm" variant="outline" on:click={copyAddress} aria-label={$t('aria.copyAddress')}>
                  <Copy class="h-3 w-3" />
                </Button>
                <Button size="sm" variant="outline" on:click={generateAndShowQrCode} title={$t('tooltips.showQr')} aria-label={$t('aria.showQr')}>
                  <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5h3v3H5zM5 16h3v3H5zM16 5h3v3h-3zM16 16h3v3h-3zM10.5 5h3M10.5 19h3M5 10.5v3M19 10.5v3M10.5 10.5h3v3h-3z"/></svg>
                </Button>
                {#if showQrCodeModal}
                  <div
                    class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
                    role="button"
                    tabindex="0"
                    on:click={() => showQrCodeModal = false}
                    on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') showQrCodeModal = false; }}
                  >
                    <div
                      class="bg-white dark:bg-gray-800 p-8 rounded-xl shadow-2xl w-full max-w-md text-center border border-gray-200 dark:border-gray-700"
                      on:click|stopPropagation
                      role="dialog"
                      tabindex="0"
                      aria-modal="true"
                      on:keydown={(e) => { if (e.key === 'Escape') showQrCodeModal = false; }}
                    >
                      <div class="flex items-center justify-center gap-2 mb-4">
                        <div class="bg-purple-100 p-2 rounded-lg">
                          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-purple-600"><path d="M5 5h3v3H5zM5 16h3v3H5zM16 5h3v3h-3zM16 16h3v3h-3zM10.5 5h3M10.5 19h3M5 10.5v3M19 10.5v3M10.5 10.5h3v3h-3z"/></svg>
                        </div>
                        <h3 class="text-xl font-bold">{$t('wallet.qrModal.title')}</h3>
                      </div>
                      
                      <div class="bg-white p-4 rounded-lg border-2 border-purple-200 shadow-sm inline-block mb-4">
                        <img src={qrCodeDataUrl} alt={$t('wallet.qrModal.alt')} class="mx-auto rounded" />
                      </div>
                      
                      <div class="bg-gray-100 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg p-3 mb-4">
                        <p class="text-xs text-gray-600 dark:text-gray-300 break-all font-mono">
                        {$etcAccount?.address}
                      </p>
                      </div>

                      <Button class="w-full font-semibold" variant="outline" on:click={() => showQrCodeModal = false}>
                        {$t('actions.close')}
                      </Button>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
            
            <div class="mt-4">
              <p class="text-sm text-muted-foreground">{$t('wallet.privateKey')}</p>
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
                  {$t('wallet.export')}
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
    <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">{$t('transfer.title')}</h2>
      <Coins class="h-5 w-5 text-muted-foreground" />
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
            <div class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4">
              <div class="bg-white dark:bg-gray-800 p-6 rounded-xl shadow-2xl w-full max-w-md border border-gray-200 dark:border-gray-700">
                <div class="flex items-center justify-center gap-2 mb-4">
                  <div class="bg-purple-100 p-2 rounded-lg">
                    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-purple-600"><path d="M5 5h3v3H5zM5 16h3v3H5zM16 5h3v3h-3zM16 16h3v3h-3zM10.5 5h3M10.5 19h3M5 10.5v3M19 10.5v3M10.5 10.5h3v3h-3z"/></svg>
                  </div>
                  <h3 class="text-xl font-bold">{$t('transfer.recipient.scanQrTitle')}</h3>
                </div>
                
                <div id="qr-reader" class="w-full border-2 border-purple-200 rounded-lg overflow-hidden"></div>
                
                <Button class="mt-4 w-full font-semibold" variant="outline" on:click={() => showScannerModal = false}>
                  {$t('actions.cancel')}
                </Button>
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
              {$t('transfer.available', { values: { amount: $wallet.balance.toFixed(2) } })}
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
                class="px-4 py-2 text-xs font-medium transition-colors {feePreset === 'low' ? 'bg-gray-800 text-white' : 'bg-white text-gray-700 hover:bg-gray-50'}" 
                on:click={() => feePreset = 'low'}
              >
                {$t('transfer.fees.low')}
              </button>
              <button 
                type="button" 
                class="px-4 py-2 text-xs font-medium border-l border-gray-300 dark:border-gray-600 transition-colors {feePreset === 'market' ? 'bg-gray-800 text-white' : 'bg-white text-gray-700 hover:bg-gray-50'}" 
                on:click={() => feePreset = 'market'}
              >
                {$t('transfer.fees.market')}
              </button>
              <button 
                type="button" 
                class="px-4 py-2 text-xs font-medium border-l border-gray-300 dark:border-gray-600 transition-colors {feePreset === 'fast' ? 'bg-gray-800 text-white' : 'bg-white text-gray-700 hover:bg-gray-50'}" 
                on:click={() => feePreset = 'fast'}
              >
                {$t('transfer.fees.fast')}
              </button>
            </div>
            <p class="text-xs text-muted-foreground mt-2">
              <span class="font-medium">Fee:</span> {estimatedFeeDisplay}
            </p>
          </div>
        
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

        <Button type="button" class="w-full justify-center border border-orange-200 dark:border-orange-800 hover:bg-orange-50 dark:hover:bg-orange-950/30 text-gray-800 dark:text-gray-200 rounded transition-colors py-2 font-medium" on:click={() => showPending = !showPending} aria-label={$t('transfer.viewPending')}>
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
  <Card class="p-6 mt-4">
    <div class="flex items-center justify-between mb-2">
      <h2 class="text-lg font-semibold">{$t('transactions.title')}</h2>
      <History class="h-5 w-5 text-muted-foreground" />
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
            title="Clear search"
            aria-label="Clear search"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
          </button>
        {/if}
      </div>
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

  <div class="flex-1"></div>

  <div class="flex flex-col gap-1 items-end">
    <button
      type="button"
      class="border border-red-300 dark:border-red-700 rounded-md px-4 py-2 text-sm h-9 bg-red-50 hover:bg-red-100 text-red-700 dark:text-red-400 transition-colors font-medium flex items-center gap-2"
      on:click={() => {
        filterType = 'transactions';
        filterDateFrom = '';
        filterDateTo = '';
        sortDescending = true;
        searchQuery = '';
      }}
    >
      <RefreshCw class="h-3.5 w-3.5" />
      {$t('filters.reset')}
    </button>
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
                  Mining rewards scanned up to block #{$miningPagination.oldestBlockScanned.toLocaleString()}
                </p>
              </div>
            {:else if !$miningPagination.hasMore && $miningPagination.oldestBlockScanned !== null}
              <div class="text-center py-3 border-t">
                <p class="text-sm text-green-600">✓ All mining rewards loaded</p>
                <p class="text-xs text-muted-foreground mt-1">
                  Scanned all blocks from #0 to current
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
  <Card class="p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="bg-blue-100 p-2 rounded-lg">
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-blue-600"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>
          </div>
        <div>
          <h2 class="text-lg font-semibold">{$t('security.2fa.title')}</h2>
            <p class="text-sm text-muted-foreground">{$t('security.2fa.subtitle_clear')}</p>
        </div>
        </div>
      </div>
      <div class="space-y-4">
        {#if is2faEnabled}
          <div class="flex items-center justify-between p-4 bg-green-50 border-l-4 border-l-green-500 border-y border-r border-green-200 rounded-lg shadow-sm">
            <div class="flex items-center gap-3">
              <div class="bg-green-100 p-2 rounded-full">
              <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-green-600"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
              </div>
              <div>
                <p class="font-semibold text-green-800">{$t('security.2fa.status.enabled')}</p>
                <p class="text-sm text-green-700">{$t('security.2fa.status.enabled_desc')}</p>
              </div>
            </div>
            <Button variant="destructive" on:click={disable2FA} class="font-semibold">{$t('security.2fa.disable')}</Button>
          </div>
        {:else}
          <div class="flex items-center justify-between p-4 bg-yellow-50 border-l-4 border-l-yellow-500 border-y border-r border-yellow-200 rounded-lg shadow-sm">
            <div class="flex items-center gap-3">
              <div class="bg-yellow-100 p-2 rounded-full">
                <AlertCircle class="h-5 w-5 text-yellow-600" />
              </div>
              <div>
                <p class="font-semibold text-yellow-900">Not Protected</p>
                <p class="text-sm text-yellow-700">{$t('security.2fa.status.disabled_desc')}</p>
              </div>
            </div>
            <Button on:click={setup2FA} class="bg-blue-600 hover:bg-blue-700 text-white font-semibold">{$t('security.2fa.enable')}</Button>
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
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <div class="bg-red-100 p-2 rounded-lg">
          <BadgeX class="h-5 w-5 text-red-600" />
        </div>
      <div>
        <h2 class="text-lg font-semibold">{$t('blacklist.title')}</h2>
          <p class="text-sm text-muted-foreground">{$t('blacklist.subtitle')}</p>
      </div>
      </div>
    </div>

    <div class="space-y-6">
      <div class="border border-gray-200 dark:border-gray-700 rounded-lg p-4">
        <div class="flex items-center gap-2 mb-3">
          <div class="bg-red-100 p-1.5 rounded">
            <Plus class="h-4 w-4 text-red-600" />
          </div>
          <h3 class="text-md font-semibold">{$t('blacklist.add.title')}</h3>
        </div>
        <div class="space-y-4">
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
                  aria-label="Clear search"
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
                        aria-label="Copy address"
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
      class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
      role="button"
      tabindex="0"
      on:click={() => show2faSetupModal = false}
      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') show2faSetupModal = false; }}
    >
      <div
        class="bg-white dark:bg-gray-800 p-6 rounded-xl shadow-2xl w-full max-w-lg border border-gray-200 dark:border-gray-700"
        on:click|stopPropagation
        role="dialog"
        aria-modal="true"
        tabindex="-1"
        on:keydown={(e) => { if (e.key === 'Escape') show2faSetupModal = false; }}
      >
        <!-- Header with Icon -->
        <div class="flex items-center gap-3 mb-4">
          <div class="bg-blue-100 p-2 rounded-lg">
            <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-blue-600"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>
          </div>
          <div>
            <h3 class="text-xl font-bold">{$t('security.2fa.setup.title')}</h3>
            <p class="text-xs text-blue-600 font-medium">Step 1 of 2</p>
          </div>
        </div>
        
        <div class="bg-blue-50 border-l-4 border-l-blue-500 p-3 rounded-lg mb-4">
          <p class="text-sm text-blue-900">{$t('security.2fa.setup.step1_scan')}</p>
        </div>
        
        <div class="flex flex-col md:flex-row gap-4 items-center border border-gray-200 dark:border-gray-700 p-4 rounded-lg mb-4">
          <div class="bg-white p-3 rounded-lg border-2 border-blue-200 shadow-sm">
            <img src={totpSetupInfo.qrCodeDataUrl} alt="2FA QR Code" class="w-40 h-40 rounded" />
          </div>
          <div class="space-y-2 flex-1">
            <p class="text-sm font-medium">{$t('security.2fa.setup.scanAlt')}</p>
            <p class="text-xs text-muted-foreground">{$t('security.2fa.setup.step2_manual')}</p>
            <div class="flex items-center gap-2 bg-gray-100 dark:bg-gray-700 p-2 rounded border border-gray-300 dark:border-gray-600">
              <code class="text-sm font-mono break-all text-blue-600 dark:text-blue-400 flex-1">{totpSetupInfo.secret}</code>
              <Button size="icon" variant="ghost" on:click={() => { navigator.clipboard.writeText(totpSetupInfo?.secret || ''); showToast(tr('toasts.common.copied'), 'success'); }} class="flex-shrink-0">
                <Copy class="h-4 w-4" />
              </Button>
            </div>
          </div>
        </div>

        <div class="bg-purple-50 border-l-4 border-l-purple-500 p-3 rounded-lg mb-4">
          <p class="text-xs text-purple-900 font-medium mb-1">Step 2 of 2</p>
          <p class="text-sm text-purple-700">{$t('security.2fa.setup.step3_verify')}</p>
        </div>
        
        <div class="space-y-3">
          <div>
            <Label for="totp-verify" class="text-sm font-semibold">{$t('security.2fa.setup.verifyLabel')}</Label>
          <Input
            id="totp-verify"
            type="text"
            bind:value={totpVerificationCode}
            placeholder="123456"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength={6}
              class="text-center text-lg font-mono tracking-wider mt-1"
          />
          </div>
          <div>
            <Label for="totp-password-setup" class="text-sm font-semibold">{$t('keystore.load.password')}</Label>
          <Input
            id="totp-password-setup"
            type="password"
            bind:value={twoFaPassword}
            placeholder={$t('placeholders.unlockPassword')}
              class="mt-1"
          />
          </div>
          {#if twoFaErrorMessage}
            <div class="bg-red-50 border-l-4 border-l-red-500 p-3 rounded-lg">
              <p class="text-sm text-red-700 flex items-center gap-2">
                <AlertCircle class="h-4 w-4" />
                {twoFaErrorMessage}
              </p>
            </div>
          {/if}
        </div>

        <div class="mt-6 flex justify-end gap-2">
          <Button variant="outline" on:click={() => show2faSetupModal = false}>{$t('actions.cancel')}</Button>
          <Button on:click={verifyAndEnable2FA} disabled={isVerifying2fa || totpVerificationCode.length < 6 || !twoFaPassword} class="bg-green-600 hover:bg-green-700 text-white font-semibold">
            {#if isVerifying2fa}
              <div class="flex items-center gap-2">
                <div class="animate-spin rounded-full h-4 w-4 border-2 border-white border-t-transparent"></div>
                {$t('actions.verifying')}
              </div>
            {:else}
              {$t('security.2fa.setup.verifyAndEnable')}
            {/if}
          </Button>
        </div>
      </div>
    </div>
  {/if}

  <!-- 2FA Action Prompt Modal -->
  {#if show2faPromptModal}
    <div
      class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
      role="button"
      tabindex="0"
      on:click={() => { show2faPromptModal = false; actionToConfirm = null; }}
      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { show2faPromptModal = false; actionToConfirm = null; } }}
    >
      <div
        class="bg-white dark:bg-gray-800 p-6 rounded-xl shadow-2xl w-full max-w-md border border-gray-200 dark:border-gray-700"
        on:click|stopPropagation
        role="dialog"
        aria-modal="true"
        tabindex="-1"
        on:keydown={(e) => { if (e.key === 'Escape' ) { show2faPromptModal = false; actionToConfirm = null; } }}
      >
        <div class="flex items-center gap-3 mb-4">
          <div class="bg-blue-100 p-2 rounded-lg">
            <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-blue-600"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>
          </div>
          <div>
            <h3 class="text-xl font-bold">{$t('security.2fa.prompt.title')}</h3>
            <p class="text-sm text-muted-foreground">{$t('security.2fa.prompt.enter_code')}</p>
          </div>
        </div>
        
        <div class="space-y-3">
          <div>
            <Label for="totp-action" class="text-sm font-semibold">{$t('security.2fa.prompt.label')}</Label>
          <Input
            id="totp-action"
            type="text"
            bind:value={totpActionCode}
            placeholder="123456"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength={6}
            autofocus
              class="text-center text-lg font-mono tracking-wider mt-1"
          />
          </div>
          <div>
            <Label for="totp-password-action" class="text-sm font-semibold">{$t('keystore.load.password')}</Label>
          <Input
            id="totp-password-action"
            type="password"
            bind:value={twoFaPassword}
            placeholder={$t('placeholders.unlockPassword')}
              class="mt-1"
          />
          </div>
          {#if twoFaErrorMessage}
            <div class="bg-red-50 border-l-4 border-l-red-500 p-3 rounded-lg">
              <p class="text-sm text-red-700 flex items-center gap-2">
                <AlertCircle class="h-4 w-4" />
                {twoFaErrorMessage}
              </p>
            </div>
          {/if}
        </div>

        <div class="mt-6 flex justify-end gap-2">
          <Button variant="outline" on:click={() => { show2faPromptModal = false; actionToConfirm = null; }}>{$t('actions.cancel')}</Button>
          <Button on:click={confirmActionWith2FA} disabled={isVerifyingAction || totpActionCode.length < 6 || !twoFaPassword} class="bg-blue-600 hover:bg-blue-700 text-white font-semibold">
            {#if isVerifyingAction}
              <div class="flex items-center gap-2">
                <div class="animate-spin rounded-full h-4 w-4 border-2 border-white border-t-transparent"></div>
                {$t('actions.verifying')}
              </div>
            {:else}
              {$t('actions.confirm')}
            {/if}
          </Button>
        </div>
      </div>
    </div>
  {/if}
  {#if autoLockMessage}
  <div class="fixed top-0 left-0 w-full bg-yellow-100 text-yellow-800 text-center py-2 z-50">
    {autoLockMessage}
  </div>
  {/if}
</div>
