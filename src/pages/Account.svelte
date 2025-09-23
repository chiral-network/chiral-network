<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { Wallet, Copy, ArrowUpRight, ArrowDownLeft, History, Coins, Plus, Import, BadgeX, KeyRound, FileText } from 'lucide-svelte'
  import DropDown from "$lib/components/ui/dropDown.svelte";
  import { wallet, etcAccount, blacklist} from '$lib/stores' 
  import { transactions } from '$lib/stores';
  import { derived } from 'svelte/store'
  import { invoke } from '@tauri-apps/api/core'
  import QRCode from 'qrcode'
  import { Html5QrcodeScanner as Html5QrcodeScannerClass } from 'html5-qrcode'
  import { tick } from 'svelte'
  import { onMount } from 'svelte'
  import { fade, fly } from 'svelte/transition'
  import { t, locale } from 'svelte-i18n'
  import { showToast } from '$lib/toast'
  import { get } from 'svelte/store'
  const tr = (k: string, params?: Record<string, any>) => get(t)(k, params)
  
  // HD wallet imports
  import MnemonicWizard from '$lib/components/wallet/MnemonicWizard.svelte'
  import AccountList from '$lib/components/wallet/AccountList.svelte'
  // HD helpers are used within MnemonicWizard/AccountList components

  // Transaction components
  import TransactionReceipt from '$lib/components/TransactionReceipt.svelte'


  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

  // Interfaces - Transaction is now defined in stores.ts

  // interface Transaction {
  //   id: number;
  //   type: 'sent' | 'received';
  //   amount: number;
  //   to?: string;
  //   from?: string;
  //   date: Date;
  //   description: string;
  //   status: 'pending' | 'completed';
  // }

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
  let isGethRunning = false
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


  let Html5QrcodeScanner: InstanceType<typeof Html5QrcodeScannerClass> | null = null;
  
  // Demo transactions - in real app these will be fetched from blockchain
  // const transactions = writable<Transaction[]>([
  //   { id: 1, type: 'received', amount: 50.5, from: '0x8765...4321', to: undefined, date: new Date('2024-03-15'), description: 'File purchase', status: 'completed' },
  //   { id: 2, type: 'sent', amount: 10.25, to: '0x1234...5678', from: undefined, date: new Date('2024-03-14'), description: 'Proxy service', status: 'completed' },
  //   { id: 3, type: 'received', amount: 100, from: '0xabcd...ef12', to: undefined, date: new Date('2024-03-13'), description: 'Upload reward', status: 'completed' },
  //   { id: 4, type: 'sent', amount: 5.5, to: '0x9876...5432', from: undefined, date: new Date('2024-03-12'), description: 'File download', status: 'completed' },
  // ]);

  // Enhanced validation states
  let validationWarning = '';
  let isAmountValid = true;
  let addressWarning = '';
  let isAddressValid = false;

  // Blacklist validation state
  let blacklistAddressWarning = '';
  let isBlacklistAddressValid = false;


  // Copy feedback message
  let copyMessage = '';
  let privateKeyCopyMessage = '';
   
  // Export feedback message
  let exportMessage = '';
  
  // Filtering state
  let filterType: 'all' | 'sent' | 'received' = 'all';
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

  // Fetch balance when account changes
  $: if ($etcAccount && isGethRunning) {
    fetchBalance()
  }

  // Derived filtered transactions
  $: filteredTransactions = $transactions
    .filter(tx => {
      const matchesType = filterType === 'all' || tx.type === filterType;
      const txDate = tx.date instanceof Date ? tx.date : new Date(tx.date);
      const fromOk = !filterDateFrom || txDate >= new Date(filterDateFrom + 'T00:00:00'); // the start of day
      const toOk = !filterDateTo || txDate <= new Date(filterDateTo + 'T23:59:59'); // and the end of day to include full date ranges
      
      // Search filter
      const matchesSearch = !searchQuery || 
        tx.description?.toLowerCase().includes(searchQuery.toLowerCase()) ||
        tx.to?.toLowerCase().includes(searchQuery.toLowerCase()) ||
        tx.from?.toLowerCase().includes(searchQuery.toLowerCase()) ||
        tx.id.toString().includes(searchQuery);
      
      return matchesType && fromOk && toOk && matchesSearch;
    })
    .slice()
    .sort((a, b) => {
      const dateA = a.date instanceof Date ? a.date : new Date(a.date);
      const dateB = b.date instanceof Date ? b.date : new Date(b.date);
      return sortDescending ? dateB.getTime() - dateA.getTime() : dateA.getTime() - dateB.getTime();
    });

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
        validationWarning = tr('errors.amount.insufficientWithFee', { values: { more: (inputValue + estimatedFeeNumeric - $wallet.balance).toFixed(2) } });
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
  
  
  showToast('Address copied to clipboard!', 'success')
  
  
}

  function copyPrivateKey() {
    with2FA(() => {
      const privateKeyToCopy = $etcAccount ? $etcAccount.private_key : '';
      if (privateKeyToCopy) {
        navigator.clipboard.writeText(privateKeyToCopy);
        privateKeyCopyMessage = tr('messages.copied');
      }
      else {
        privateKeyCopyMessage = tr('messages.failed');
      }
      setTimeout(() => privateKeyCopyMessage = '', 1500);
    });
  }
    
  function exportWallet() {
    with2FA(async () => {
      try {
        const walletData = {
          address: $etcAccount?.address,
          privateKey: $etcAccount?.private_key,
          balance: $wallet.balance,
          totalEarned: $wallet.totalEarned,
          totalSpent: $wallet.totalSpent,
          pendingTransactions: $wallet.pendingTransactions,
          exportDate: new Date().toISOString(),
          version: "1.0"
        };
        
        const dataStr = JSON.stringify(walletData, null, 2);
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
    showToast('Transaction cancelled', 'warning')
  }

  function sendTransaction() {
    if (!isAddressValid || !isAmountValid || !isAddressValid || sendAmount <= 0) return
    
    // Notify submission (mocked)
    showToast('Transaction submitted', 'info')

    // Simulate transaction
    wallet.update(w => ({
      ...w,
      balance: w.balance - sendAmount,
      pendingTransactions: w.pendingTransactions + 1,
      totalSpent: w.totalSpent + sendAmount
    }))

    transactions.update(txs => [
    {
      id: Date.now(),
      type: 'sent',
      amount: sendAmount,
      to: recipientAddress,
      date: new Date(),
      description: tr('transactions.manual'),
      status: 'pending'
    },
    ...txs // prepend so latest is first
  ])
    
    // Clear form
    recipientAddress = ''
    sendAmount = 0
    rawAmountInput = ''
    
    // Simulate transaction completion
    setTimeout(() => {
      wallet.update(w => ({
        ...w,
        pendingTransactions: Math.max(0, w.pendingTransactions - 1)
      }))
      transactions.update(txs => txs.map(tx => tx.status === 'pending' ? { ...tx, status: 'completed' } : tx))
      // Notify success (mocked)
      showToast('Transaction confirmed', 'success')
    }, 3000)
  }
  
  function formatDate(date: Date): string {
    const loc = get(locale) || 'en-US'
    return new Intl.DateTimeFormat(loc, { month: 'short', day: 'numeric', year: 'numeric' }).format(date)
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

  let balanceInterval: number | undefined
  
  onMount(() => {
    checkGethStatus()
    loadKeystoreAccountsList();

    // Set up periodic balance refresh every 10 seconds
    balanceInterval = window.setInterval(() => {
      if ($etcAccount && isGethRunning) {
        fetchBalance()
      }
    }, 10000)

    // Cleanup function
    return () => {
      if (balanceInterval) window.clearInterval(balanceInterval)
    }
  })

  async function checkGethStatus() {
    try {
      if (isTauri) {
        isGethRunning = await invoke('is_geth_running') as boolean
        // Fetch balance if account exists and geth is running
        if ($etcAccount && isGethRunning) {
          fetchBalance()
        }
      } else {
        // Fallback for web environment - assume geth is not running
        isGethRunning = false
        console.log('Running in web mode - geth not available')
      }
    } catch (error) {
      console.error('Failed to check geth status:', error)
    }
  }

  async function fetchBalance() {
    if (!$etcAccount) return
    
    try {
      if (isTauri && isGethRunning) {
        // Desktop app with local geth node - get real blockchain balance
        const balance = await invoke('get_account_balance', { address: $etcAccount.address }) as string
        wallet.update(w => ({ ...w, balance: parseFloat(balance) }))
      } else if (isTauri && !isGethRunning) {
        // Desktop app but geth not running - use stored balance
        console.log('Geth not running - using stored balance')
      } else {
        // Web environment - For now, simulate balance updates for demo purposes
        const simulatedBalance = $wallet.balance + Math.random() * 10 // Small random changes
        wallet.update(w => ({ ...w, balance: Math.max(0, simulatedBalance) }))
      }
    } catch (error) {
      console.error('Failed to fetch balance:', error)
      // Fallback to stored balance on error
    }
  }

  
  async function createChiralAccount() {
  isCreatingAccount = true
  try {
    let account: { address: string, private_key: string, blacklist: Object[] }

    if (isTauri) {
      account = await invoke('create_chiral_account') as { address: string, private_key: string, blacklist: Object[] }
    } else {
      const demoAddress = '0x' + Math.random().toString(16).substr(2, 40)
      const demoPrivateKey = '0x' + Math.random().toString(16).substr(2, 64)
      const demoBlackList = [{node_id: 169245, name: "Jane"}]
      account = {
        address: demoAddress,
        private_key: demoPrivateKey,
        blacklist: demoBlackList
      }
      console.log('Running in web mode - using demo account')
    }
    
    etcAccount.set(account)
    wallet.update(w => ({
      ...w,
      address: account.address
    }))
    
    showToast('Account Created Successfully!', 'success')
    
    if (isGethRunning) {
      await fetchBalance()
    }
  } catch (error) {
    console.error('Failed to create Chiral account:', error)
    showToast('Failed to create account: ' + String(error), 'error')
    alert(tr('errors.createAccount', { error: String(error) }))
  } finally {
    isCreatingAccount = false
  }
}

  async function setAccount(account: { address: string, private_key: string }) {
    etcAccount.set(account);
    wallet.update(w => ({ ...w, address: account.address }));
    if (isGethRunning) { await fetchBalance(); }
  }

  async function saveToKeystore() {
    if (!keystorePassword || !$etcAccount) return;

    isSavingToKeystore = true;
    keystoreSaveMessage = '';

    try {
        if (isTauri) {
            await invoke('save_account_to_keystore', {
                address: $etcAccount.address,
                privateKey: $etcAccount.private_key,
                password: keystorePassword,
            });
            keystoreSaveMessage = tr('keystore.success');
        } else {
            // Simulate for web
            console.log('Simulating save to keystore with password:', keystorePassword);
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
    function onScanSuccess(decodedText: string, decodedResult: any) {
      // Handle the scanned code
      console.log(`Code matched = ${decodedText}`, decodedResult);
      
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
    
    isImportingAccount = true
    try {
      let account: { address: string, private_key: string }
      
      if (isTauri) {
        account = await invoke('import_chiral_account', { privateKey: importPrivateKey }) as { address: string, private_key: string }
      } else {
        const demoAddress = '0x' + Math.random().toString(16).substr(2, 40)
        account = {
          address: demoAddress,
          private_key: importPrivateKey
        }
        console.log('Running in web mode - using provided private key')
      }
      
      etcAccount.set(account)
      wallet.update(w => ({
        ...w,
        address: account.address
      }))
      await setAccount(account);
      importPrivateKey = ''
      
      
      showToast('Account imported successfully!', 'success')
      
      if (isGethRunning) {
        await fetchBalance()
      }
    } catch (error) {
      console.error('Failed to import Chiral account:', error)
      
      
      showToast('Failed to import account: ' + String(error), 'error')
      
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
            showToast('Invalid file format: privateKey field not found', 'error');
            return;
          }
          
          // Extract and set the private key
          importPrivateKey = accountData.privateKey;
          showToast('Private key loaded from file successfully!', 'success');
          
        } catch (error) {
          console.error('Error reading file:', error);
          showToast('Error reading file: ' + String(error), 'error');
        }
      };
      
      // Trigger file selection
      document.body.appendChild(fileInput);
      fileInput.click();
      document.body.removeChild(fileInput);
      
    } catch (error) {
      console.error('Error loading file:', error);
      showToast('Error loading file: ' + String(error), 'error');
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
    // set as active
    etcAccount.set({ address: ev.account.address, private_key: '0x' + ev.account.privateKeyHex });
    wallet.update(w => ({ ...w, address: ev.account.address }));
    if (isGethRunning) { await fetchBalance(); }
  }
  function onHDAccountsChange(updated: HDAccountItem[]) {
    hdAccounts = updated;
  }

  async function loadKeystoreAccountsList() {
    try {
      if (isTauri) {
        const accounts = await invoke('list_keystore_accounts') as string[];
        keystoreAccounts = accounts;
        if (accounts.length > 0) {
          selectedKeystoreAccount = accounts[0];
        }
      }
    } catch (error) {
      console.error('Failed to list keystore accounts:', error);
    }
  }

  async function loadFromKeystore() {
    if (!selectedKeystoreAccount || !loadKeystorePassword) return;

    isLoadingFromKeystore = true;
    keystoreLoadMessage = '';

    try {
        if (isTauri) {
            // Send password to backend for decryption
            const decryptedAccount = await invoke('load_account_from_keystore', {
                address: selectedKeystoreAccount,
                password: loadKeystorePassword,
            }) as { 
                address: string, 
                private_key: string,
            };

            // Verify the decrypted address matches selected address
            if (decryptedAccount.address.toLowerCase() !== selectedKeystoreAccount.toLowerCase()) {
                throw new Error(tr('keystore.load.addressMismatch'));
            }

            // Update stores with decrypted account
            etcAccount.set({
                address: decryptedAccount.address,
                private_key: decryptedAccount.private_key
            });

            // Update wallet store
            wallet.update(w => ({
                ...w,
                address: decryptedAccount.address
            }));
            await setAccount(decryptedAccount);
            
            // Clear sensitive data
            loadKeystorePassword = '';
            
            // Fetch initial balance if geth is running
            if (isGethRunning) {
                await fetchBalance();
            }
            
            keystoreLoadMessage = tr('keystore.load.success');

        } else {
            // Web demo mode simulation
            console.log('Simulating keystore load in web mode');
            await new Promise(resolve => setTimeout(resolve, 1000));
            keystoreLoadMessage = tr('keystore.load.successSimulated');
        }
        
    } catch (error) {
        console.error('Failed to load from keystore:', error);
        keystoreLoadMessage = tr('keystore.load.error', { error: String(error) });
        
        // Clear sensitive data on error
        loadKeystorePassword = '';
    } finally {
        isLoadingFromKeystore = false;
        setTimeout(() => keystoreLoadMessage = '', 4000);
    }
  }

  // --- 2FA Functions ---

  // This would be called by the "Enable 2FA" button
  async function setup2FA() {
    // In a real app, this would come from the Rust backend
    // RUST: `invoke('generate_totp_secret')` -> { secret: string, otpauth_url: string }
    try {
      // --- SIMULATION ---
      // This is a dummy secret. Your backend should generate a cryptographically secure one.
      const secret = 'JBSWY3DPEHPK3PXP' + Math.random().toString(36).substring(2, 10).toUpperCase();
      const issuer = encodeURIComponent('Chiral Network');
      const account = encodeURIComponent($etcAccount?.address.slice(0, 16) || 'user');
      const otpauth_url = `otpauth://totp/${issuer}:${account}?secret=${secret}&issuer=${issuer}`;
      const qrCodeDataUrl = await QRCode.toDataURL(otpauth_url);
      // --- END SIMULATION ---

      totpSetupInfo = { secret, qrCodeDataUrl };
      show2faSetupModal = true;
      totpVerificationCode = '';
      twoFaErrorMessage = '';
    } catch (err) {
      console.error('Failed to setup 2FA:', err);
      showToast('Failed to start 2FA setup.', 'error');
    }
  }

  // Called from the setup modal to verify and enable 2FA
  async function verifyAndEnable2FA() {
    if (!totpSetupInfo || !totpVerificationCode) return;
    isVerifying2fa = true;
    twoFaErrorMessage = '';

    try {
      // RUST: `invoke('verify_and_enable_totp', { secret: totpSetupInfo.secret, code: totpVerificationCode })` -> bool
      // --- SIMULATION: We'll accept any 6-digit code for this demo. ---
      await new Promise(r => setTimeout(r, 500));
      const success = /^\d{6}$/.test(totpVerificationCode);
      if (!success) throw new Error('Invalid verification code format.');
      // --- END SIMULATION ---

      if (success) {
        // Persist that 2FA is enabled for the account.
        is2faEnabled = true; 
        show2faSetupModal = false;
        showToast('Two-Factor Authentication has been enabled!', 'success');
      } else {
        twoFaErrorMessage = 'Invalid code. Please try again.';
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
      // RUST: `invoke('verify_totp_code', { code: totpActionCode })` -> bool
      // --- SIMULATION: We'll accept any 6-digit code for this demo. ---
      await new Promise(r => setTimeout(r, 500));
      const success = /^\d{6}$/.test(totpActionCode);
      if (!success) throw new Error('Invalid 2FA code.');
      // --- END SIMULATION ---

      if (success) {
        show2faPromptModal = false;
        actionToConfirm(); // Execute the original action
      } else {
        twoFaErrorMessage = 'Invalid code. Please try again.';
      }
    } catch (error) {
      twoFaErrorMessage = String(error);
    } finally {
      isVerifyingAction = false;
      actionToConfirm = null;
    }
  }

  // To disable 2FA (this action is also protected by 2FA)
  function disable2FA() {
      with2FA(() => {
          // RUST: `invoke('disable_2fa')`
          // --- SIMULATION ---
          is2faEnabled = false;
          showToast('Two-Factor Authentication has been disabled.', 'warning');
          // --- END SIMULATION ---
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
  function addBlacklistEntry() {
    if (!isBlacklistFormValid) return;
    const newEntry = { chiral_address: newBlacklistEntry.chiral_address, reason: newBlacklistEntry.reason, timestamp: new Date() };
    blacklist.update(entries => [...entries, newEntry]);
    // Clear input fields
    newBlacklistEntry.chiral_address = "";
    newBlacklistEntry.reason = "";
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

  function logout() {
    // Clear the account details from memory, effectively logging out
    etcAccount.set(null);

    // Reset wallet state to defaults
    wallet.update(w => ({
      ...w,
      address: '',
      balance: 0,
      totalEarned: 0,
      totalSpent: 0,
      pendingTransactions: 0,
    }));

    // Explicitly nullify sensitive component state variables to assist garbage collection.
    privateKeyVisible = false;
    keystorePassword = '';
    loadKeystorePassword = '';
    importPrivateKey = '';

    // For enhanced security, clear any session-related data from browser storage.
    // This helps ensure no sensitive information like private keys persists in localStorage.
    // Note: This will clear ALL data for this domain (e.g., settings, blacklist).
    if (typeof window !== 'undefined') {
      window.localStorage?.clear();
      window.sessionStorage?.clear();
    }

    console.log('Session cleared, wallet locked.');
    showToast('Wallet locked and session data cleared', 'success');
    
    // Refresh the list of keystore accounts for the login view
    loadKeystoreAccountsList();
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
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('account.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('account.subtitle')}</p>
</div>

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
        <div>
          <p class="text-sm text-muted-foreground">{$t('wallet.balance')}</p>
          <p class="text-2xl font-bold">{$wallet.balance.toFixed(2)} Chiral</p>
        </div>
        
            <div class="grid grid-cols-2 gap-4 mt-4">
          <div>
            <p class="text-xs text-muted-foreground">{$t('wallet.totalEarned')}</p>
            <p class="text-sm font-medium text-green-600">+{$wallet.totalEarned.toFixed(2)} Chiral</p>
          </div>
          <div>
            <p class="text-xs text-muted-foreground">{$t('wallet.totalSpent')}</p>
            <p class="text-sm font-medium text-red-600">-{$wallet.totalSpent.toFixed(2)} Chiral</p>
          </div>
        </div>
        
            <div class="mt-6">
              <p class="text-sm text-muted-foreground">{$t('wallet.address')}</p>
              <div class="flex items-center gap-2 mt-1">
                <p class="font-mono text-sm">{$etcAccount.address.slice(0, 10)}...{$etcAccount.address.slice(-8)}</p>
                <div class="relative">
                  <Button size="sm" variant="outline" on:click={copyAddress} aria-label={$t('aria.copyAddress')}>
                    <Copy class="h-3 w-3" />
                  </Button>
                  {#if copyMessage}
                    <span class="absolute top-full left-1/2 transform -translate-x-1/2 text-xs text-green-600 mt-1 whitespace-nowrap">{copyMessage}</span>
                  {/if}
                </div>
                <Button size="sm" variant="outline" on:click={generateAndShowQrCode} title={$t('tooltips.showQr')} aria-label={$t('aria.showQr')}>
                  <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5h3v3H5zM5 16h3v3H5zM16 5h3v3h-3zM16 16h3v3h-3zM10.5 5h3M10.5 19h3M5 10.5v3M19 10.5v3M10.5 10.5h3v3h-3z"/></svg>
                </Button>
                {#if showQrCodeModal}
                  <div
                    class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
                    role="button"
                    tabindex="0"
                    on:click={() => showQrCodeModal = false}
                    on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') showQrCodeModal = false; }}
                  >
                    <div
                      class="bg-white p-8 rounded-lg shadow-xl w-full max-w-xs text-center"
                      on:click|stopPropagation
                      role="dialog"
                      tabindex="0"
                      aria-modal="true"
                      on:keydown={(e) => { if (e.key === 'Escape') showQrCodeModal = false; }}
                    >
                      <h3 class="text-lg font-semibold mb-4">{$t('wallet.qrModal.title')}</h3>
                      
                      <img src={qrCodeDataUrl} alt={$t('wallet.qrModal.alt')} class="mx-auto rounded-md border" />
                      
                      <p class="text-xs text-gray-600 mt-4 break-all font-mono">
                        {$etcAccount?.address}
                      </p>

                      <Button class="mt-6 w-full" variant="outline" on:click={() => showQrCodeModal = false}>
                        {$t('actions.close')}
                      </Button>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
            
            <div class="mt-4">
              <p class="text-sm text-muted-foreground">{$t('wallet.privateKey')}</p>
                <div class="flex gap-2 mt-1">
                  <Input
                    type="text"
                    value={privateKeyVisible ? $etcAccount.private_key : '•'.repeat($etcAccount.private_key.length)}
                    readonly
                    class="flex-1 font-mono text-xs min-w-0"
                  />
                <div class="relative">
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={copyPrivateKey}
                    aria-label={$t('aria.copyPrivateKey')}
                  >
                    <Copy class="h-3 w-3" />
                  </Button>
                  {#if privateKeyCopyMessage}
                    <span class="absolute top-full left-1/2 transform -translate-x-1/2 text-xs text-green-600 mt-1 whitespace-nowrap">{privateKeyCopyMessage}</span>
                  {/if}
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  class="w-16"
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
                <Button type="button" variant="destructive" on:click={logout}>
                  {$t('actions.lockWallet')}
                </Button>
              </div>
              {#if exportMessage}<p class="text-xs text-center mt-2 {exportMessage.includes('successfully') ? 'text-green-600' : 'text-red-600'}">{exportMessage}</p>{/if}
            </div>
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
              class="pr-10" 
              data-form-type="other"
              data-lpignore="true"
              aria-autocomplete="none"
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              class="absolute right-1 top-1/2 -translate-y-1/2 h-8 w-8 p-0"
              on:click={scanQrCode}
              aria-label={$t('transfer.recipient.scanQr')}
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"></rect><rect x="14" y="3" width="7" height="7"></rect><rect x="3" y="14" width="7" height="7"></rect><line x1="14" x2="14" y1="14" y2="21"></line><line x1="21" x2="21" y1="14" y2="21"></line><line x1="21" x2="14" y1="21" y2="21"></line></svg>
            </Button>
          </div>
          {#if showScannerModal}
            <div class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4">
              <div class="bg-white p-6 rounded-lg shadow-xl w-full max-w-md">
                <h3 class="text-lg font-semibold mb-4 text-center">{$t('transfer.recipient.scanQrTitle')}</h3>
                
                <div id="qr-reader" class="w-full"></div>
                
                <Button class="mt-4 w-full" variant="outline" on:click={() => showScannerModal = false}>
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
              class="mt-2"
              data-form-type="other"
              data-lpignore="true"
              aria-autocomplete="none"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              class="absolute right-1 top-1/2 transform -translate-y-1/2 h-8 px-3"
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
          
          <!-- Fee selector (UI stub) -->
          <div class="mt-3">
            <div class="inline-flex rounded-md border overflow-hidden">
              <button type="button" class="px-3 py-1 text-xs {feePreset === 'low' ? 'bg-foreground text-background' : 'bg-background'}" on:click={() => feePreset = 'low'}>{$t('fees.low')}</button>
              <button type="button" class="px-3 py-1 text-xs border-l {feePreset === 'market' ? 'bg-foreground text-background' : 'bg-background'}" on:click={() => feePreset = 'market'}>{$t('fees.market')}</button>
              <button type="button" class="px-3 py-1 text-xs border-l {feePreset === 'fast' ? 'bg-foreground text-background' : 'bg-background'}" on:click={() => feePreset = 'fast'}>{$t('fees.fast')}</button>
            </div>
            <p class="text-xs text-muted-foreground mt-2">{$t('fees.estimated')}: {estimatedFeeDisplay}</p>
          </div>
        
        </div>

        <Button
          type="button"
          class="w-full"
          on:click={handleSendClick}
          disabled={!isAddressValid || !isAmountValid || rawAmountInput === ''}>
          <ArrowUpRight class="h-4 w-4 mr-2" />
          {#if isConfirming}
            {$t('transfer.sendingIn', { values: { seconds: countdown } })}
          {:else}
            {$t('transfer.send')}
          {/if}
        </Button>

        <Button type="button" class="w-full justify-center bg-gray-100 hover:bg-gray-200 text-gray-800 rounded transition-colors py-2 font-normal" on:click={() => showPending = !showPending} aria-label={$t('transfer.viewPending')}>
          <span class="flex items-center gap-2">
            <svg class="h-4 w-4 text-orange-500" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24">
              <circle cx="12" cy="10" r="8" />
              <polyline points="12,6 12,10 16,14" />
            </svg>
            {$t('transfer.pending.count', { values: { count: $pendingCount } })}
          </span>
        </Button>
        {#if showPending}
          <div class="mt-2 p-3 bg-gray-50 rounded shadow">
            <h3 class="text-sm mb-2 text-gray-700 font-normal">{$t('transfer.pending.title')}</h3>
            <ul class="space-y-1">
              {#each $transactions.filter(tx => tx.status === 'pending') as tx}
                <li class="text-xs text-gray-800 font-normal">
                  {tx.description} ({tx.type === 'sent' ? $t('transactions.item.to') : $t('transactions.item.from')}: {tx.type === 'sent' ? tx.to : tx.from}) - {tx.amount} Chiral
                </li>
              {:else}
                <li class="text-xs text-gray-500 font-normal">{$t('transfer.pending.noDetails')}</li>
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
          <p class="text-sm text-muted-foreground mb-4">Path m/44'/{98765}'/0'/0/*</p>
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
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">{$t('transactions.title')}</h2>
      <History class="h-5 w-5 text-muted-foreground" />
    </div>
    
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
          class="w-full pl-10 pr-4 py-2 border rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        />
      </div>
    </div>

    <!-- Filters -->
    <div class="flex flex-wrap gap-4 mb-4 items-end">
      <div>
        <label for="filter-type" class="block text-xs font-medium mb-1">{$t('filters.type')}</label>
        <select id="filter-type" bind:value={filterType} class="border rounded px-2 py-1 text-sm">
          <option value="all">{$t('filters.typeAll')}</option>
          <option value="sent">{$t('filters.typeSent')}</option>
          <option value="received">{$t('filters.typeReceived')}</option>
        </select>
      </div>
      <div>
        <label for="filter-date-from" class="block text-xs font-medium mb-1">{$t('filters.from')}</label>
        <input id="filter-date-from" type="date" bind:value={filterDateFrom} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label for="filter-date-to" class="block text-xs font-medium mb-1">{$t('filters.to')}</label>
        <input id="filter-date-to" type="date" bind:value={filterDateTo} class="border rounded px-2 py-1 text-sm" />
      </div>
      <div>
        <label for="sort-button" class="block text-xs font-medium mb-1">{$t('filters.sort')}</label>
        <button id="sort-button" type="button" class="border rounded px-3 py-1 text-sm bg-white hover:bg-gray-100 transition-colors w-full" on:click={() => { sortDescending = !sortDescending; }} aria-pressed={sortDescending}>
          {sortDescending ? $t('filters.sortNewest') : $t('filters.sortOldest')}
        </button>
      </div>
      <div class="flex-1"></div>
      <div class="flex flex-col gap-1 items-end">
        <button type="button" class="border rounded px-3 py-1 text-sm bg-muted hover:bg-muted/70 transition-colors" on:click={() => { filterType = 'all'; filterDateFrom = ''; filterDateTo = ''; sortDescending = true; searchQuery = ''; }}>
          {$t('filters.reset')}
        </button>
      </div>
    </div>

    <!-- Transaction List -->
    <div class="space-y-2 max-h-80 overflow-y-auto pr-1">
      {#each filteredTransactions as tx}
        <div 
          class="flex items-center justify-between p-3 bg-secondary rounded-lg hover:bg-secondary/80 cursor-pointer transition-colors"
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
          <div class="flex items-center gap-3">
            {#if tx.type === 'received'}
              <ArrowDownLeft class="h-4 w-4 text-green-500" />
            {:else}
              <ArrowUpRight class="h-4 w-4 text-red-500" />
            {/if}
            <div>
              <p class="text-sm font-medium">{tx.description}</p>
              <p class="text-xs text-muted-foreground">
                {tx.type === 'received' ? $t('transactions.item.from') : $t('transactions.item.to')}: {tx.type === 'received' ? tx.from : tx.to}
              </p>
            </div>
          </div>
          <div class="text-right">
            <p class="text-sm font-medium {tx.type === 'received' ? 'text-green-600' : 'text-red-600'}">
              {tx.type === 'received' ? '+' : '-'}{tx.amount} Chiral
            </p>
            <p class="text-xs text-muted-foreground">{formatDate(tx.date)}</p>
          </div>
        </div>
      {/each}
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
        <div>
          <h2 class="text-lg font-semibold">{$t('security.2fa.title')}</h2>
          <p class="text-sm text-muted-foreground mt-1">{$t('security.2fa.subtitle')}</p>
        </div>
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-muted-foreground"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>
      </div>
      <div class="space-y-4">
        {#if is2faEnabled}
          <div class="flex items-center justify-between p-3 bg-green-50 border border-green-200 rounded-lg">
            <div class="flex items-center gap-3">
              <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-green-600"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>
              <div>
                <p class="font-semibold text-green-800">{$t('security.2fa.enabledTitle')}</p>
                <p class="text-sm text-green-700">{$t('security.2fa.enabledDesc')}</p>
              </div>
            </div>
            <Button variant="destructive" on:click={disable2FA}>{$t('security.2fa.disable')}</Button>
          </div>
        {:else}
          <div class="flex items-center justify-between p-4 border-2 border-dashed rounded-lg">
            <p class="text-sm text-muted-foreground">{$t('security.2fa.disabledDesc')}</p>
            <Button on:click={setup2FA}>{$t('security.2fa.enable')}</Button>
          </div>
        {/if}
        <p class="text-xs text-muted-foreground">{$t('security.2fa.explanation')}</p>
      </div>
  </Card>
  {/if}

  <Card class="p-6">
    <div class="flex items-center gap-2 mb-4">
      <KeyRound class="h-5 w-5 text-muted-foreground" />
      <h2 class="text-lg font-semibold">{$t('keystore.title')}</h2>
    </div>
    <div class="space-y-4">
      <p class="text-sm text-muted-foreground">
        {$t('keystore.desc')}
      </p>
      <div class="flex items-center gap-2">
        <div class="flex-1">
          <Input
            type="password"
            bind:value={keystorePassword}
            placeholder={$t('placeholders.password')}
            class="w-full {passwordStrength ? `border-${passwordStrength === 'strong' ? 'green' : passwordStrength === 'medium' ? 'yellow' : 'red'}-500` : ''}"
            autocomplete="new-password"
          />
          {#if keystorePassword}
            <div class="mt-1 flex items-center gap-2">
              <div class="h-1 flex-1 bg-gray-200 rounded-full overflow-hidden">
                <div
                  class="h-full transition-all duration-300 {passwordStrength === 'strong' ? 'bg-green-500 w-full' : passwordStrength === 'medium' ? 'bg-yellow-500 w-2/3' : 'bg-red-500 w-1/3'}"
                ></div>
              </div>
              <span class="text-xs {passwordStrength === 'strong' ? 'text-green-600' : passwordStrength === 'medium' ? 'text-yellow-600' : 'text-red-600'}">
                {passwordFeedback}
              </span>
            </div>
            <ul class="text-xs text-muted-foreground mt-2 space-y-1">
              <li class="{keystorePassword.length >= 8 ? 'text-green-600' : ''}">• {$t('password.requirements.length')}</li>
              <li class="{/[A-Z]/.test(keystorePassword) ? 'text-green-600' : ''}">• {$t('password.requirements.uppercase')}</li>
              <li class="{/[a-z]/.test(keystorePassword) ? 'text-green-600' : ''}">• {$t('password.requirements.lowercase')}</li>
              <li class="{/[0-9]/.test(keystorePassword) ? 'text-green-600' : ''}">• {$t('password.requirements.number')}</li>
              <li class="{/[!@#$%^&*(),.?":{}|<>]/.test(keystorePassword) ? 'text-green-600' : ''}">• {$t('password.requirements.special')}</li>
            </ul>
          {/if}
        </div>
        <Button
          on:click={saveToKeystore}
          disabled={!isPasswordValid || isSavingToKeystore}
        >
          {#if isSavingToKeystore}
            {$t('actions.saving')}
          {:else}
            {$t('actions.saveKey')}
          {/if}
        </Button>
      </div>
      {#if keystoreSaveMessage}
        <p class="text-xs text-center mt-2 {keystoreSaveMessage.toLowerCase().includes('success') ? 'text-green-600' : 'text-red-600'}">{keystoreSaveMessage}</p>
      {/if}
    </div>
  </Card>

  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div>
        <h2 class="text-lg font-semibold">{$t('blacklist.title')}</h2>
        <p class="text-sm text-muted-foreground mt-1">{$t('blacklist.subtitle')}</p>
      </div>
      <BadgeX class="h-5 w-5 text-muted-foreground" />
    </div>

    <div class="space-y-6">
      <div class="border rounded-lg p-4 bg-gray-50/50">
        <h3 class="text-md font-medium mb-3">{$t('blacklist.add.title')}</h3>
        <div class="space-y-4">
          <div>
            <Label for="blacklist-address">{$t('blacklist.add.address')}</Label>
            <Input
              id="blacklist-address"
              bind:value={newBlacklistEntry.chiral_address}
              placeholder={$t('blacklist.add.addressPlaceholder')}
              class="mt-2 font-mono text-sm {isBlacklistAddressValid ? 'border-green-300' : ''}"
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

          <div class="flex flex-wrap items-center gap-2">
            <span class="text-xs text-muted-foreground">{$t('blacklist.quickReasons.label')}</span>
            {#each [$t('blacklist.quickReasons.spam'), $t('blacklist.quickReasons.fraud'), $t('blacklist.quickReasons.malicious'), $t('blacklist.quickReasons.harassment'), $t('blacklist.quickReasons.scam')] as reason}
              <button
                type="button"
                class="px-2 py-1 text-xs border rounded hover:bg-gray-100 transition-colors"
                on:click={() => newBlacklistEntry.reason = reason}
              >
                {reason}
              </button>
            {/each}
          </div>

          <Button 
            type="button" 
            class="w-full" 
            disabled={!isBlacklistFormValid} 
            on:click={addBlacklistEntry}
          >
            {$t('blacklist.add.submit')}
          </Button>
        </div>
      </div>

      <div>
        <div class="flex items-center justify-between mb-3">
          <div class="flex items-center gap-2">
            <h3 class="text-md font-medium">{$t('blacklist.list.title')}</h3>
            {#if $blacklist.length > 0}
              <span class="text-sm text-muted-foreground">({$blacklist.length})</span>
            {/if}
          </div>
          
          <div class="flex gap-2">
            <div class="relative">
              <Input
                bind:value={blacklistSearch}
                placeholder={$t('placeholders.searchBlacklist')}
                class="w-96 text-sm pr-8"
              />
              {#if blacklistSearch}
                <button
                  type="button"
                  class="absolute right-2 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground"
                  on:click={clearBlacklistSearch}
                  title={$t('tooltips.clearSearch')}
                >
                  ×
                </button>
              {:else}
                <div class="absolute right-2 top-1/2 transform -translate-y-1/2 text-muted-foreground pointer-events-none">
                  <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <circle cx="11" cy="11" r="8"/>
                    <path d="m21 21-4.35-4.35"/>
                  </svg>
                </div>
              {/if}
            </div>
            
            {#if $blacklist.length > 0}
              <Button 
                size="sm" 
                variant="outline" 
                on:click={clearAllBlacklist}
                class="text-red-600 hover:text-red-700"
              >
                {$t('blacklist.actions.clearAll')}
              </Button>
            {/if}
          </div>
        </div>

        {#if filteredBlacklist.length === 0 && $blacklist.length === 0}
          <div class="text-center py-8 text-muted-foreground border-2 border-dashed border-gray-200 rounded-lg">
            <BadgeX class="h-12 w-12 mx-auto mb-2 opacity-20" />
            <p class="font-medium">{$t('blacklist.list.emptyTitle')}</p>
            <p class="text-sm mt-1">{$t('blacklist.list.emptyDesc')}</p>
          </div>
        {:else if filteredBlacklist.length === 0 && blacklistSearch}
          <div class="text-center py-6 text-muted-foreground">
            <p>{$t('blacklist.list.noMatch', { values: { q: blacklistSearch } })}</p>
          </div>
        {:else}
          <div class="space-y-2 max-h-64 overflow-y-auto">
            {#each filteredBlacklist as entry, index (entry.chiral_address)}
              <div class="flex items-center justify-between p-3 bg-red-50 border border-red-100 rounded-lg group hover:bg-red-100/50 transition-colors">
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2 mb-1">
                    <p class="text-sm font-mono font-medium truncate">
                      {entry.chiral_address}
                    </p>
                    <button
                      type="button"
                      class="opacity-0 group-hover:opacity-100 transition-opacity p-1 hover:bg-red-200 rounded"
                      on:click={() => copyToClipboard(entry.chiral_address)}
                      title={$t('blacklist.actions.copyAddress')}
                    >
                      <Copy class="h-3 w-3" />
                    </button>
                  </div>

                  {#if editingEntry === index}
                    <div class="space-y-2">
                      <Input
                        bind:value={editReason}
                        placeholder={$t('placeholders.reason')}
                        maxlength={200}
                        class="text-xs"
                        on:keydown={handleEditKeydown}
                        autofocus
                      />
                      <div class="flex gap-2">
                        <Button size="sm" on:click={saveEdit} disabled={!editReason.trim()}>
                          {$t('actions.save')}
                        </Button>
                        <Button size="sm" variant="outline" on:click={cancelEdit}>
                          {$t('actions.cancel')}
                        </Button>
                      </div>
                    </div>
                  {:else}
                    <p class="text-xs text-muted-foreground mb-1">{entry.reason}</p>
                  {/if}
                  
                  <p class="text-xs text-muted-foreground">
                    {$t('blacklist.list.addedAt', { values: { date: formatDate(entry.timestamp) } })}
                  </p>
                </div>
                
                <div class="flex items-center gap-2 ml-4">
                  {#if editingEntry !== index}
                    <Button 
                      size="sm" 
                      variant="outline"
                      on:click={() => startEditEntry(index)}
                      class="opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      {$t('actions.edit')}
                    </Button>
                  {/if}
                  
                  <Button 
                    size="sm" 
                    variant="destructive"
                    on:click={() => removeBlacklistEntry(entry.chiral_address)}
                    disabled={editingEntry === index}
                  >
                    {$t('actions.remove')}
                  </Button>
                </div>
              </div>
            {/each}
          </div>
          
          {#if $blacklist.length > 5}
            <div class="text-center mt-3">
              <p class="text-xs text-muted-foreground">
                {$t('blacklist.list.showing', { values: { shown: filteredBlacklist.length, total: $blacklist.length } })}
              </p>
            </div>
          {/if}
        {/if}
      </div>

        <div class="border-t pt-4">
          <div class="flex gap-2">
            {#if $blacklist.length > 0}
            <Button 
              variant="outline" 
              size="sm" 
              on:click={exportBlacklist}
              class="flex-1"
              disabled={$blacklist.length === 0}
              title={$t('blacklist.actions.exportTitle', { values: { count: $blacklist.length } })}
            >
              {$t('blacklist.actions.export')} {$blacklist.length > 0 ? `(${$blacklist.length})` : ''}
            </Button>
            {/if}
            <Button 
              variant="outline" 
              size="sm" 
              on:click={() => importFileInput.click()}
              class="flex-1"
            >
              {$t('blacklist.actions.import')}
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

  <!-- Transaction Receipt Modal -->
  <TransactionReceipt
    transaction={selectedTransaction}
    isOpen={showTransactionReceipt}
    onClose={closeTransactionReceipt}
  />

  <!-- 2FA Setup Modal -->
  {#if show2faSetupModal && totpSetupInfo}
    <div
      class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
      role="button"
      tabindex="0"
      on:click={() => show2faSetupModal = false}
      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') show2faSetupModal = false; }}
    >
      <div
        class="bg-card p-6 rounded-lg shadow-xl w-full max-w-md text-card-foreground"
        on:click|stopPropagation
        role="dialog"
        aria-modal="true"
        on:keydown={(e) => { if (e.key === 'Escape') show2faSetupModal = false; }}
      >
        <h3 class="text-xl font-semibold mb-2">{$t('security.2fa.setup.title')}</h3>
        <p class="text-sm text-muted-foreground mb-4">{$t('security.2fa.setup.step1')}</p>
        
        <div class="flex flex-col md:flex-row gap-4 items-center bg-background p-4 rounded-lg">
          <img src={totpSetupInfo.qrCodeDataUrl} alt="2FA QR Code" class="w-40 h-40 rounded-md border bg-white p-1" />
          <div class="space-y-2">
            <p class="text-sm">{$t('security.2fa.setup.scanAlt')}</p>
            <p class="text-xs text-muted-foreground">{$t('security.2fa.setup.manualLabel')}</p>
            <div class="flex items-center gap-2 bg-secondary p-2 rounded">
              <code class="text-sm font-mono break-all">{totpSetupInfo.secret}</code>
              <Button size="icon" variant="ghost" on:click={() => { navigator.clipboard.writeText(totpSetupInfo.secret); showToast('Copied!', 'success'); }}>
                <Copy class="h-4 w-4" />
              </Button>
            </div>
          </div>
        </div>

        <p class="text-sm text-muted-foreground my-4">{$t('security.2fa.setup.step2')}</p>
        <div class="space-y-2">
          <Label for="totp-verify">{$t('security.2fa.setup.verifyLabel')}</Label>
          <Input
            id="totp-verify"
            type="text"
            bind:value={totpVerificationCode}
            placeholder="123456"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="6"
          />
          {#if twoFaErrorMessage}
            <p class="text-sm text-red-500">{twoFaErrorMessage}</p>
          {/if}
        </div>

        <div class="mt-6 flex justify-end gap-2">
          <Button variant="outline" on:click={() => show2faSetupModal = false}>{$t('actions.cancel')}</Button>
          <Button on:click={verifyAndEnable2FA} disabled={isVerifying2fa || totpVerificationCode.length < 6}>
            {isVerifying2fa ? $t('actions.verifying') : $t('security.2fa.setup.verifyAndEnable')}
          </Button>
        </div>
      </div>
    </div>
  {/if}

  <!-- 2FA Action Prompt Modal -->
  {#if show2faPromptModal}
    <div
      class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
      role="button"
      tabindex="0"
      on:click={() => { show2faPromptModal = false; actionToConfirm = null; }}
      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { show2faPromptModal = false; actionToConfirm = null; } }}
    >
      <div
        class="bg-card p-6 rounded-lg shadow-xl w-full max-w-sm text-card-foreground"
        on:click|stopPropagation
        role="dialog"
        aria-modal="true"
        on:keydown={(e) => { if (e.key === 'Escape') { show2faPromptModal = false; actionToConfirm = null; } }}
      >
        <h3 class="text-xl font-semibold mb-2">{$t('security.2fa.prompt.title')}</h3>
        <p class="text-sm text-muted-foreground mb-4">{$t('security.2fa.prompt.subtitle')}</p>
        
        <div class="space-y-2">
          <Label for="totp-action">{$t('security.2fa.prompt.label')}</Label>
          <Input
            id="totp-action"
            type="text"
            bind:value={totpActionCode}
            placeholder="123456"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="6"
            autofocus
          />
          {#if twoFaErrorMessage}
            <p class="text-sm text-red-500">{twoFaErrorMessage}</p>
          {/if}
        </div>

        <div class="mt-6 flex justify-end gap-2">
          <Button variant="outline" on:click={() => { show2faPromptModal = false; actionToConfirm = null; }}>{$t('actions.cancel')}</Button>
          <Button on:click={confirmActionWith2FA} disabled={isVerifyingAction || totpActionCode.length < 6}>
            {isVerifyingAction ? $t('actions.verifying') : $t('actions.confirm')}
          </Button>
        </div>
      </div>
    </div>
  {/if}

</div>
