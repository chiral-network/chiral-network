<script lang="ts">
  import { goto } from '@mateothegreat/svelte5-router';
</script>

<div class="flex flex-col items-center justify-center h-full p-6 text-center">
  <h1 class="text-3xl font-bold mb-2">404 - Page Not Found</h1>
  <p class="text-gray-500 mb-6">
    The page you're looking for doesn't exist or has been moved.
  </p>

  <button
    class="px-4 py-2 rounded bg-indigo-600 text-white hover:bg-indigo-700"
    on:click={() => goto('/download')}
  >
    Go to Download
  </button>
</div>
