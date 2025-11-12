<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Dropdown from '$lib/components/ui/dropDown.svelte'
  import { deriveNext } from '$lib/wallet/hd'
  import { showToast } from '$lib/toast'
  import { wallet, etcAccount } from '$lib/stores'
  import { walletService } from '$lib/wallet'

  interface AccountItem { index: number; change: number; address: string; label?: string; privateKeyHex?: string }
  export let mnemonic: string
  export let passphrase: string
  export let accounts: AccountItem[] = []
  export let onAccountsChange: (a: AccountItem[]) => void

  let renameIndex: number | null = null
  let renameValue = ''

  let selectedAccountIndex: number = 0;
  $: dropdownOptions = accounts.map((acc, i) => ({
    value: String(i),
    label: `${acc.label || 'Account ' + acc.index} - ${short(acc.address, 4, 4)}`
  }));
  $: selectedAccount = accounts[selectedAccountIndex] ?? null;

  function handleDropdownChange(event: CustomEvent<{ value: string }>) {
    selectedAccountIndex = Number(event.detail.value);
    renameIndex = null;
    renameValue = '';
  }

  function short(addr: string, prefix = 10, suffix = 8): string {
    if (!addr) return ''
    if (addr.length <= prefix + suffix + 3) return addr
    return `${addr.slice(0, prefix)}…${addr.slice(-suffix)}`
  }

  async function selectAccount(acc: AccountItem) {
    // Import to backend to set as active account
    const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
    if (isTauri && acc.privateKeyHex) {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const privateKeyWithPrefix = acc.privateKeyHex.startsWith('0x') ? acc.privateKeyHex : '0x' + acc.privateKeyHex
        await invoke('import_chiral_account', { privateKey: privateKeyWithPrefix })
      } catch (error) {
        console.error('Failed to set backend account:', error)
      }
    }
    
    etcAccount.set({ address: acc.address, private_key: acc.privateKeyHex || '' })
    wallet.update(w => ({ ...w, address: acc.address }))
    showToast('Selected account ' + acc.address.slice(0, 10) + '...', 'success')
  }

  async function addNext() {
    try {
      const derived = await deriveNext(mnemonic, passphrase, accounts, 0)
      const item: AccountItem = { index: derived.index, change: 0, address: derived.address, privateKeyHex: derived.privateKeyHex }
      const updated = [...accounts, item]
      onAccountsChange(updated)
      if (updated.length > 3) {
        selectedAccountIndex = updated.length - 1;
      }
      showToast('Derived account #' + item.index, 'success')
    } catch (e) {
      showToast('Derivation failed: ' + String(e), 'error')
    }
  }

  async function saveToKeystore(acc: AccountItem) {
    try {
      if (!acc.privateKeyHex) return
      const pk = acc.privateKeyHex.startsWith('0x') ? acc.privateKeyHex : acc.privateKeyHex
      const password = prompt('Enter keystore password') || ''
      if (!password) return
      await walletService.saveToKeystore(password, { address: acc.address, private_key: pk })
      showToast('Saved to keystore', 'success')
    } catch (e) {
      showToast('Keystore save failed: ' + String(e), 'error')
    }
  }

  function startRename(acc: AccountItem) {
    renameIndex = acc.index
    renameValue = acc.label || ''
  }
  function commitRename(acc: AccountItem) {
    const updated = accounts.map(a => a.index === acc.index ? { ...a, label: renameValue } : a)
    onAccountsChange(updated)
    renameIndex = null
    renameValue = ''
  }
</script>

<Card class="p-4 space-y-3">
  <div class="grid grid-cols-1 sm:grid-cols-2 gap-2 items-center">
    <h3 class="font-semibold">Accounts</h3>
    <div class="w-full sm:w-auto sm:justify-self-end">
      <Button class="w-full sm:w-auto" on:click={addNext}>Derive Next</Button>
    </div>
  </div>

  {#if accounts.length > 3}
    <div class="space-y-2">
      <Dropdown
        options={dropdownOptions}
        value={String(selectedAccountIndex)}
        on:change={handleDropdownChange}
      />
    </div>

    {#if selectedAccount}
      {@const acc = selectedAccount}
      <div class="border rounded-md p-3 space-y-3">
        <!-- Top: name + address -->
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="text-sm text-muted-foreground">#{acc.index}</span>
            {#if renameIndex === acc.index}
              <Input class="h-8 text-sm w-full sm:w-48" bind:value={renameValue} on:keydown={(e) => { const ev = (e as unknown as KeyboardEvent); if (ev.key === 'Enter') commitRename(acc) }} />
            {:else}
              <span class="font-medium truncate">{acc.label || 'Account ' + acc.index}</span>
            {/if}
          </div>
          <div class="mt-1 text-xs text-muted-foreground font-mono min-w-0" title={acc.address}>
            <!-- Responsive middle truncation to keep hash readable without overflow -->
            <span class="sm:hidden">{short(acc.address, 6, 4)}</span>
            <span class="hidden sm:inline md:hidden">{short(acc.address, 8, 6)}</span>
            <span class="hidden md:inline">{short(acc.address, 10, 8)}</span>
          </div>
        </div>
        <!-- Bottom: buttons -->
        <div class="flex flex-wrap gap-2 w-full">
          {#if renameIndex === acc.index}
            <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" variant="outline" on:click={() => commitRename(acc)}>Save</Button>
          {:else}
            <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" variant="outline" on:click={() => startRename(acc)}>Rename</Button>
          {/if}
          <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" variant="outline" on:click={() => saveToKeystore(acc)}>
            <span class="hidden lg:inline">Save to Keystore</span>
            <span class="hidden md:inline lg:hidden">Save Key</span>
            <span class="md:hidden">Save</span>
          </Button>
          <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" on:click={() => selectAccount(acc)} disabled={acc.address === $wallet.address}>Select</Button>
        </div>
      </div>
    {/if}
  {:else}
    <div class="space-y-2">
      {#each accounts as acc}
        <div class="border rounded-md p-3 space-y-3">
          <!-- Top: name + address -->
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-sm text-muted-foreground">#{acc.index}</span>
              {#if renameIndex === acc.index}
                <Input class="h-8 text-sm w-full sm:w-48" bind:value={renameValue} on:keydown={(e) => { const ev = (e as unknown as KeyboardEvent); if (ev.key === 'Enter') commitRename(acc) }} />
              {:else}
                <span class="font-medium truncate">{acc.label || 'Account ' + acc.index}</span>
              {/if}
            </div>
            <div class="mt-1 text-xs text-muted-foreground font-mono min-w-0" title={acc.address}>
              <!-- Responsive middle truncation to keep hash readable without overflow -->
              <span class="sm:hidden">{short(acc.address, 6, 4)}</span>
              <span class="hidden sm:inline md:hidden">{short(acc.address, 8, 6)}</span>
              <span class="hidden md:inline">{short(acc.address, 10, 8)}</span>
            </div>
          </div>
          <!-- Bottom: buttons -->
          <div class="flex flex-wrap gap-2 w-full">
            {#if renameIndex === acc.index}
              <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" variant="outline" on:click={() => commitRename(acc)}>Save</Button>
            {:else}
              <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" variant="outline" on:click={() => startRename(acc)}>Rename</Button>
            {/if}
            <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" variant="outline" on:click={() => saveToKeystore(acc)}>
              <span class="hidden lg:inline">Save to Keystore</span>
              <span class="hidden md:inline lg:hidden">Save Key</span>
              <span class="md:hidden">Save</span>
            </Button>
            <Button class="w-full xs:w-auto whitespace-nowrap text-xs h-8 px-2" size="sm" on:click={() => selectAccount(acc)} disabled={acc.address === $wallet.address}>Select</Button>
          </div>
        </div>
      {/each}
      {#if accounts.length === 0}
        <p class="text-sm text-muted-foreground">No derived accounts yet.</p>
      {/if}
    </div>
  {/if}
</Card>
