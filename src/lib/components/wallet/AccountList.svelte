<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import { deriveNext } from '$lib/wallet/hd'
  import { showToast } from '$lib/toast'
  import { wallet, etcAccount } from '$lib/stores'
  import { invoke } from '@tauri-apps/api/core'

  interface AccountItem { index: number; change: number; address: string; label?: string; privateKeyHex?: string }
  export let mnemonic: string
  export let passphrase: string
  export let accounts: AccountItem[] = []
  export let onAccountsChange: (a: AccountItem[]) => void

  let renameIndex: number | null = null
  let renameValue = ''

  function selectAccount(acc: AccountItem) {
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
      showToast('Derived account #' + item.index, 'success')
    } catch (e) {
      showToast('Derivation failed: ' + String(e), 'error')
    }
  }

  async function saveToKeystore(acc: AccountItem) {
    try {
      if (!acc.privateKeyHex) return
      const pk = '0x' + acc.privateKeyHex
      // Ensure address correctness from backend
      const info = await invoke<{ address: string, private_key: string }>('import_chiral_account', { privateKey: pk })
      await invoke('save_account_to_keystore', { address: info.address, privateKey: info.private_key, password: prompt('Enter keystore password') || '' })
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
  <div class="flex items-center justify-between">
    <h3 class="font-semibold">Accounts</h3>
    <Button on:click={addNext}>Derive Next</Button>
  </div>
  <div class="space-y-2">
    {#each accounts as acc}
      <div class="border rounded-md p-3 flex items-center justify-between gap-3">
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="text-sm text-muted-foreground">#{acc.index}</span>
            {#if renameIndex === acc.index}
              <Input class="h-8 text-sm" bind:value={renameValue} on:keydown={(e) => { const ev = (e as unknown as KeyboardEvent); if (ev.key === 'Enter') commitRename(acc) }} />
            {:else}
              <span class="font-medium truncate">{acc.label || 'Account ' + acc.index}</span>
            {/if}
          </div>
          <div class="text-xs text-muted-foreground truncate">{acc.address}</div>
        </div>
        <div class="flex gap-2">
          {#if renameIndex === acc.index}
            <Button size="sm" variant="outline" on:click={() => commitRename(acc)}>Save</Button>
          {:else}
            <Button size="sm" variant="outline" on:click={() => startRename(acc)}>Rename</Button>
          {/if}
          <Button size="sm" variant="outline" on:click={() => saveToKeystore(acc)}>Save to Keystore</Button>
          <Button size="sm" on:click={() => selectAccount(acc)}>Select</Button>
        </div>
      </div>
    {/each}
    {#if accounts.length === 0}
      <p class="text-sm text-muted-foreground">No derived accounts yet.</p>
    {/if}
  </div>
</Card>
