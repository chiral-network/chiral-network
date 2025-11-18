<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { createMnemonic, isValidMnemonic, deriveAccount } from '$lib/wallet/hd'
  import { onMount } from 'svelte'
  import { showToast } from '$lib/toast'
  import { t } from 'svelte-i18n'

  type TranslateParams = { values?: Record<string, unknown>; default?: string }
  // const tr = (key: string, params?: TranslateParams) => get(t)(key, params)
  const tr = (key: string, params?: TranslateParams) => $t(key, params)

  export let onComplete: (args: { mnemonic: string, passphrase: string, account: { address: string, privateKeyHex: string, index: number, change: number }, name?: string }) => void
  export let onCancel: () => void
  export let mode: 'create' | 'import' = 'create'

  let step: 1 | 2 | 3 = 1
  let mnemonic = ''
  let passphrase = ''
  let confirmWords = ['', '']
  let confirmIdxs: number[] = []
  let confirmError = ''
  let isBusy = false
  let ackChecked = false
  // Creation options: 12 or 24 words (128 or 256 bits)
  let selectedStrength: 128 | 256 = 128
  // Optional wallet name
  let walletName = ''

  onMount(async () => {
    if (mode === 'create') {
      mnemonic = await createMnemonic(selectedStrength)
      // choose two random word positions for confirmation
      const words = mnemonic.split(' ')
      const i1 = Math.floor(Math.random() * words.length)
      let i2 = Math.floor(Math.random() * words.length)
      if (i2 === i1) i2 = (i1 + 1) % words.length
      confirmIdxs = [Math.min(i1, i2), Math.max(i1, i2)]
    }
  })

  async function regenerate() {
    mnemonic = await createMnemonic(selectedStrength)
    const words = mnemonic.split(' ')
    const i1 = Math.floor(Math.random() * words.length)
    let i2 = Math.floor(Math.random() * words.length)
    if (i2 === i1) i2 = (i1 + 1) % words.length
    confirmIdxs = [Math.min(i1, i2), Math.max(i1, i2)]
    confirmWords = ['', '']
    confirmError = ''
  }

  function next() {
    if (mode === 'create' && step === 1) { confirmError = ''; step = 2; return }
    if (mode === 'create' && step === 2) { goFromConfirm(); return }
    if (mode === 'import' && step === 1) { confirmError = ''; step = 3; return }
  }

  function goFromConfirm() {
    // Ensure both confirmation words are provided
    if (!confirmWords[0]?.trim() || !confirmWords[1]?.trim()) {
      confirmError = 'Please enter both confirmation words.'
      return
    }
    const words = mnemonic.trim().split(/\s+/)
    const ok = confirmWords[0].trim() === words[confirmIdxs[0]] && confirmWords[1].trim() === words[confirmIdxs[1]]
    if (!ok) {
      confirmError = 'Confirmation words do not match.'
      return
    }
    confirmError = ''
    step = 3
  }

  async function finish() {
    try {
      isBusy = true
      if (mode === 'create') {
        // At this point, confirmation already validated in step 2.
        if (!ackChecked) { isBusy = false; return }
      } else {
        // validate mnemonic
        const wc = mnemonic.trim().split(/\s+/).filter(Boolean).length
        const allowed = [12, 24]
        if (!allowed.includes(wc)) { confirmError = 'Mnemonic must be 12 or 24 words.'; isBusy = false; return }
        const valid = await isValidMnemonic(mnemonic)
        if (!valid) { confirmError = 'Invalid mnemonic (checksum)'; isBusy = false; return }
      }
      const acct = await deriveAccount(mnemonic, passphrase || '', 0, 0)
      onComplete({ mnemonic, passphrase: passphrase || '', account: { address: acct.address, privateKeyHex: acct.privateKeyHex, index: 0, change: 0 }, name: walletName.trim() || undefined })
      // showToast('Wallet ready', 'success')
      showToast(tr('toasts.wallet.mnemonic.ready'), 'success')
    } catch (e) {
      console.error(e)
      confirmError = String(e)
    } finally {
      isBusy = false
    }
  }
</script>

<div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4">
  <Card class="w-full max-w-2xl p-6 space-y-4">
    {#if mode === 'create'}
      {#if step === 1}
        <h2 class="text-xl font-semibold">Your Recovery Phrase</h2>
        <p class="text-sm text-muted-foreground">Write these {mnemonic.split(' ').length} words down in order and store them securely. Anyone with this phrase can access your funds.</p>
        <div class="flex items-center justify-between gap-2">
          <div class="flex items-center gap-2 text-sm">
            <Label>Words</Label>
            <select class="border rounded px-2 py-1 text-sm" bind:value={selectedStrength} on:change={regenerate}>
              <option value={128}>12</option>
              <option value={256}>24</option>
            </select>
          </div>
          <div class="flex items-center gap-2">
            <!-- <Button variant="outline" on:click={async () => { await navigator.clipboard.writeText(mnemonic); showToast('Recovery phrase copied to clipboard', 'success') }}>Copy</Button> -->
             <Button variant="outline" on:click={async () => { await navigator.clipboard.writeText(mnemonic); showToast(tr('toasts.wallet.mnemonic.copied'), 'success') }}>Copy</Button>
            <Button variant="outline" on:click={regenerate}>Regenerate</Button>
          </div>
        </div>
        <div class="grid grid-cols-3 gap-2 p-3 rounded-md border bg-muted/30">
          {#each mnemonic.split(' ') as w, i}
            <div class="text-sm"><span class="text-muted-foreground mr-1">{i+1}.</span>{w}</div>
          {/each}
        </div>
        <div class="flex gap-2 justify-end">
          <Button variant="outline" on:click={onCancel}>Cancel</Button>
          <Button on:click={next}>I have written it down</Button>
        </div>
      {:else if step === 2}
        <h2 class="text-xl font-semibold">Confirm Recovery Phrase</h2>
        <p class="text-sm text-muted-foreground">Enter the word #{confirmIdxs[0]+1} and #{confirmIdxs[1]+1} to confirm.</p>
        <div class="grid grid-cols-2 gap-4">
          <div>
            <Label>Word #{confirmIdxs[0]+1}</Label>
            <Input bind:value={confirmWords[0]} placeholder="word" />
          </div>
          <div>
            <Label>Word #{confirmIdxs[1]+1}</Label>
            <Input bind:value={confirmWords[1]} placeholder="word" />
          </div>
        </div>
        <div>
          <Label>Optional Passphrase</Label>
          <Input type="password" bind:value={passphrase} placeholder="(optional) unlock phrase" />
          <p class="text-xs text-muted-foreground mt-1">Adds extra protection. Do not forget it.</p>
        </div>
        {#if confirmError}<p class="text-sm text-red-500">{confirmError}</p>{/if}
        <div class="flex gap-2 justify-end">
          <Button variant="outline" on:click={() => step = 1}>Back</Button>
          <Button on:click={next}>Continue</Button>
        </div>
      {:else}
        <h2 class="text-xl font-semibold">Acknowledgement</h2>
        <p class="text-sm text-muted-foreground mb-3">I understand that the recovery phrase and (if set) passphrase are the only way to recover my wallet.</p>
        <div class="mb-3">
          <Label>Wallet Name (optional)</Label>
          <Input bind:value={walletName} placeholder="e.g., Main Wallet" />
        </div>
        <label class="flex items-center gap-2 text-sm">
          <input type="checkbox" bind:checked={ackChecked} />
          <span>I Understand</span>
        </label>
        <div class="flex gap-2 justify-end">
          <Button variant="outline" on:click={() => step = 2}>Back</Button>
          <Button disabled={isBusy || !ackChecked} on:click={finish}>{isBusy ? 'Creating...' : 'Create Wallet'}</Button>
        </div>
        {#if confirmError}<p class="text-sm text-red-500">{confirmError}</p>{/if}
      {/if}
    {:else}
      <!-- import mode -->
      <h2 class="text-xl font-semibold">Import from Recovery Phrase</h2>
      <Label>Recovery Phrase (12 or 24 words)</Label>
      <textarea class="w-full border rounded-md p-2" rows="4" bind:value={mnemonic} placeholder="enter words separated by spaces"></textarea>
      <Label>Optional Passphrase</Label>
      <Input type="password" bind:value={passphrase} placeholder="(optional) unlock phrase" />
      {#if confirmError}<p class="text-sm text-red-500">{confirmError}</p>{/if}
      <div class="flex gap-2 justify-end">
        <Button variant="outline" on:click={onCancel}>Cancel</Button>
        <Button disabled={isBusy} on:click={finish}>{isBusy ? 'Importing...' : 'Import'}</Button>
      </div>
    {/if}
  </Card>
</div>

<style>
  textarea { background: var(--background); color: var(--foreground); }
</style>
