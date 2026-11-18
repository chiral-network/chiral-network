<script lang="ts">
  import { versionStatus, type VersionStatus } from '$lib/stores/versionStore';
  import { ExternalLink, AlertTriangle, X } from 'lucide-svelte';
  import { onDestroy } from 'svelte';
  import { toasts } from '$lib/toastStore';

  let status = $state<VersionStatus | null>(null);
  let dismissedRecommended = $state(false);

  const unsubscribe = versionStatus.subscribe((v) => {
    status = v;
  });
  onDestroy(unsubscribe);

  // Restore "dismissed for this session" so the banner doesn't keep
  // re-appearing after every re-poll inside the same app run.
  if (typeof window !== 'undefined' && window.sessionStorage) {
    dismissedRecommended = window.sessionStorage.getItem('chiral-update-banner-dismissed') === '1';
  }

  function dismissRecommended() {
    dismissedRecommended = true;
    if (typeof window !== 'undefined' && window.sessionStorage) {
      window.sessionStorage.setItem('chiral-update-banner-dismissed', '1');
    }
  }

  // Only an HTTPS URL without credentials is offered for copying (book A.2).
  function httpsDownload(value: string | undefined): string | null {
    try {
      const url = new URL(value ?? '');
      return url.protocol === 'https:' && !url.username && !url.password ? url.href : null;
    } catch { return null; }
  }
  const downloadUrl = $derived(httpsDownload(status?.policy.downloadUrl));

  async function openDownload() {
    if (!downloadUrl) return;
    try {
      await navigator.clipboard.writeText(downloadUrl);
      toasts.show('Download URL copied. Open it in your browser to update.', 'info');
    } catch {
      toasts.show('Copy the displayed HTTPS download URL into your browser.', 'warning');
    }
  }

  // The required-update modal takes focus and keeps Tab inside it; the application
  // shell behind it is made inert by App.svelte.
  let modal = $state<HTMLElement>();
  $effect(() => {
    if (status?.status === 'required' && modal && !modal.contains(document.activeElement)) {
      (modal.querySelector<HTMLElement>('button, a[href], [tabindex]:not([tabindex="-1"])') ?? modal).focus();
    }
  });
  function trapFocus(event: KeyboardEvent) {
    if (event.key !== 'Tab' || !modal) return;
    const focusable = [...modal.querySelectorAll<HTMLElement>('button, a[href], [tabindex]:not([tabindex="-1"])')];
    if (!focusable.length) { event.preventDefault(); modal.focus(); return; }
    const first = focusable[0], last = focusable[focusable.length - 1];
    if (event.shiftKey && (document.activeElement === first || document.activeElement === modal)) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
  }
</script>

{#if status}
  {#if status.status === 'required'}
    <!--
      Hard block: full-screen modal covering everything. Pointer events
      cannot pass through. The user's only path out is to update.
    -->
    <div
      bind:this={modal}
      class="fixed inset-0 z-[2147483646] flex items-center justify-center bg-black/80 backdrop-blur-sm"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="update-required-title"
      tabindex="-1"
      onkeydown={trapFocus}
    >
      <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl border border-red-200 dark:border-red-700 max-w-md w-full mx-4 p-6">
        <div class="flex items-center gap-3 mb-4">
          <div class="p-2 bg-red-100 dark:bg-red-900/30 rounded-lg">
            <AlertTriangle class="w-6 h-6 text-red-600 dark:text-red-400" />
          </div>
          <h2 id="update-required-title" class="text-lg font-semibold text-gray-900 dark:text-white">
            Update required
          </h2>
        </div>
        <p class="text-sm text-gray-700 dark:text-gray-300 mb-3">
          This client (<code class="font-mono">v{status.currentVersion}</code>) is below the
          network's required minimum version
          (<code class="font-mono">v{status.policy.minRequired}</code>). Discovery and
          paid network operations are disabled until you update.
        </p>
        {#if status.policy.message}
          <p class="text-sm text-gray-600 dark:text-gray-400 mb-4 italic">
            {status.policy.message}
          </p>
        {/if}
        {#if downloadUrl}
          <button
            type="button"
            onclick={openDownload}
            class="w-full px-4 py-2.5 bg-red-600 hover:bg-red-700 text-white rounded-lg flex items-center justify-center gap-2 font-medium transition"
          >
            <ExternalLink class="w-4 h-4" />
            Copy download URL
          </button>
          <p class="text-xs text-gray-400 dark:text-gray-500 mt-3 text-center break-all">{downloadUrl}</p>
        {:else}
          <p class="text-sm text-gray-700 dark:text-gray-300">This policy gives no HTTPS download URL. Obtain the update from your distribution channel.</p>
        {/if}
      </div>
    </div>
  {:else if status.status === 'recommended' && !dismissedRecommended}
    <!--
      Soft nudge: dismissable top-of-screen banner. Sits above page
      content but below modals.
    -->
    <div
      class="fixed top-0 inset-x-0 z-[60] bg-amber-50 dark:bg-amber-900/30 border-b border-amber-200 dark:border-amber-700 px-4 py-2 flex items-center gap-3"
      role="status"
    >
      <AlertTriangle class="w-4 h-4 text-amber-600 dark:text-amber-400 flex-shrink-0" />
      <span class="text-sm text-amber-900 dark:text-amber-200 flex-1">
        Update available — running <code class="font-mono">v{status.currentVersion}</code>,
        recommended <code class="font-mono">v{status.policy.recommended}</code>.
        {#if status.policy.message}
          {status.policy.message}
        {/if}
      </span>
      {#if downloadUrl}
        <button
          type="button"
          onclick={openDownload}
          class="text-sm text-amber-900 dark:text-amber-200 underline hover:no-underline whitespace-nowrap"
        >
          Update
        </button>
      {/if}
      <button
        type="button"
        onclick={dismissRecommended}
        aria-label="Dismiss"
        class="p-1 rounded hover:bg-amber-100 dark:hover:bg-amber-900/50"
      >
        <X class="w-4 h-4 text-amber-700 dark:text-amber-300" />
      </button>
    </div>
  {/if}
{/if}
