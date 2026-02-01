<script lang="ts">
  import { walletAccount } from '$lib/stores';
</script>

<div class="p-6">
  <h1 class="text-3xl font-bold mb-6">Account</h1>
  
  {#if $walletAccount}
    <div class="bg-white rounded-lg shadow p-6">
      <h2 class="text-xl font-semibold mb-4">Wallet Address</h2>
      <p class="font-mono text-sm bg-gray-50 p-3 rounded border border-gray-200">
        {$walletAccount.address}
      </p>
    </div>
  {/if}
</div>
