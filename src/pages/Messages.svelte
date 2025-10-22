<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Mail, Send, Search, RefreshCw } from "lucide-svelte";
  import Button from "$lib/components/ui/button.svelte";
  import Input from "$lib/components/ui/input.svelte";
  import { t } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { formatDistanceToNow } from 'date-fns';

  type Peer = {
    peerId: string;
    address: string;
    lastSeen: number;
  };

  let conversations: Peer[] = [];
  let selectedPeerId: string | null = null;
  let messages = [];
  let newMessage = "";
  let messageContainer: HTMLElement;
  let isLoadingPeers = false;

  function selectConversation(peerId: string) {
    selectedPeerId = peerId;
    messages = [
      { from: "them", text: "Hey, how is it going?" },
      { from: "me", text: "Pretty good, working on the new messaging feature." },
      { from: "them", text: "Nice! How is it coming along?" },
      { from: "me", text: "Almost there. Just need to wire up the UI." },
      { from: "them", text: "See you tomorrow!" },
    ];
    newMessage = "";
    scrollToBottom();
  }

  function sendMessage() {
    if (!newMessage.trim()) return;
    messages = [...messages, { from: "me", text: newMessage }];
    newMessage = "";
    scrollToBottom();
  }

  async function scrollToBottom() {
    await tick();
    if (messageContainer) {
      messageContainer.scrollTop = messageContainer.scrollHeight;
    }
  }

  async function fetchPeers() {
    isLoadingPeers = true;
    try {
      const peers: Peer[] = await invoke("get_peer_metrics");
      // Sort peers by last seen, most recent first
      peers.sort((a, b) => b.lastSeen - a.lastSeen);
      conversations = peers;

      // If no conversation is selected, or the selected one is no longer in the list, select the first one.
      if (conversations.length > 0 && (!selectedPeerId || !conversations.some(p => p.peerId === selectedPeerId))) {
        selectConversation(conversations[0].peerId);
      } else if (conversations.length === 0) {
        selectedPeerId = null;
      }
    } catch (error) {
      console.error("Failed to fetch peers:", error);
      // You could show a toast notification here
    } finally {
      isLoadingPeers = false;
    }
  }

  onMount(() => {
    fetchPeers();
  });
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <div class="mb-6">
    <h1 class="text-3xl font-bold">{$t('messages.title', { default: 'Messages' })}</h1>
    <p class="text-muted-foreground mt-2">{$t('messages.subtitle', { default: 'Communicate securely with your peers.' })}</p>
  </div>

  <!-- Main Content Area -->
  <div class="flex flex-1 min-h-0 border rounded-lg bg-card text-card-foreground overflow-hidden">
    <!-- Conversation List -->
    <div class="w-1/3 border-r flex flex-col">
      <div class="p-4 border-b bg-card flex justify-between items-center">
        <h2 class="text-lg font-semibold flex items-center gap-2">{$t('messages.conversations', { default: 'Contacts' })}</h2>
        <Button variant="ghost" size="icon" on:click={fetchPeers} disabled={isLoadingPeers} aria-label="Refresh peer list">
          <RefreshCw class="h-4 w-4 {isLoadingPeers ? 'animate-spin' : ''}" />
        </Button>
      </div>
      <div class="p-4 border-b">
        <div class="relative mt-3">
          <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input placeholder={$t('messages.searchPlaceholder', { default: 'Search contacts...' })} class="pl-9" />
        </div>
      </div>
      <div class="overflow-y-auto flex-1">
        {#each conversations as conv}
          <div
            class="p-4 cursor-pointer hover:bg-muted/50 border-b transition-colors flex items-center gap-3 {selectedPeerId === conv.peerId ? 'bg-muted' : ''}"
            on:click={() => selectConversation(conv.peerId)}
            on:keydown={(e) => e.key === 'Enter' && selectConversation(conv.peerId)}
            role="button"
            tabindex="0"
          >
            <div class="relative flex-shrink-0">
              <div class="h-12 w-12 rounded-full bg-muted flex items-center justify-center text-lg font-semibold">
                {conv.peerId.slice(-2, -1).toUpperCase()}
              </div>
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex justify-between items-center mb-1">
                <p class="font-semibold truncate" title={conv.peerId}>
                  Peer <span class="font-mono text-sm">{conv.peerId.slice(0, 8)}...{conv.peerId.slice(-4)}</span>
                </p>
                <p class="text-xs text-muted-foreground flex-shrink-0 ml-2">{formatDistanceToNow(new Date(conv.lastSeen * 1000), { addSuffix: true })}</p>
              </div>
              <div class="flex justify-between items-center">
                <p class="text-sm text-muted-foreground truncate flex-1" title={conv.address}>
                  {conv.address.split('/p2p/')[0]}
                </p>
              </div>
            </div>
          </div>
        {/each}
      </div>
    </div>

    <!-- Message View -->
    <div class="w-2/3 flex flex-col bg-background text-card-foreground">
      {#if selectedPeerId}
        <!-- Chat Header -->
        <div class="p-4 border-b bg-card flex items-center gap-3">
          <div class="h-10 w-10 rounded-full bg-muted flex items-center justify-center font-semibold">
            {selectedPeerId.slice(-2, -1).toUpperCase()}
          </div>
          <div>
            <p class="font-semibold" title={selectedPeerId}>
              Peer <span class="font-mono text-sm">{selectedPeerId.slice(0, 12)}...{selectedPeerId.slice(-6)}</span>
            </p>
            <p class="text-xs text-muted-foreground">End-to-end encrypted</p>
          </div>
        </div>

        <!-- Messages Area -->
        <div bind:this={messageContainer} class="flex-1 p-6 overflow-y-auto space-y-4">
          {#each messages as msg, i (i)}
            <div class="flex {msg.from === 'me' ? 'justify-end' : 'justify-start'}">
              <div class="max-w-[70%] px-4 py-2.5 rounded-lg {msg.from === 'me' ? 'bg-primary text-primary-foreground' : 'bg-muted'}">
                <p class="text-sm">{msg.text}</p>
              </div>
            </div>
          {/each}
        </div>

        <!-- Input Area -->
        <div class="p-4 border-t bg-card">
          <form on:submit|preventDefault={sendMessage} class="flex items-center gap-3">
            <Input 
              bind:value={newMessage} 
              placeholder={$t('messages.inputPlaceholder', { default: 'Type an encrypted message...' })} 
              class="flex-1" 
              autocomplete="off" 
            />
            <Button type="submit" disabled={!newMessage.trim()} class="flex-shrink-0">
              <Send class="h-4 w-4" />
            </Button>
          </form>
        </div>
      {:else}
        <div class="flex-1 flex flex-col items-center justify-center text-muted-foreground p-6">
          <Mail class="h-16 w-16 mb-4 opacity-50" />
          <p class="text-lg">{$t('messages.selectConversation', { default: 'Select a conversation to start messaging' })}</p>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  /* Ensure smooth scrolling */
  :global(.overflow-y-auto) {
    scrollbar-width: thin;
    scrollbar-color: hsl(var(--muted-foreground) / 0.3) transparent;
  }

  :global(.overflow-y-auto::-webkit-scrollbar) {
    width: 6px;
  }

  :global(.overflow-y-auto::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(.overflow-y-auto::-webkit-scrollbar-thumb) {
    background-color: hsl(var(--muted-foreground) / 0.3);
    border-radius: 3px;
  }

  :global(.overflow-y-auto::-webkit-scrollbar-thumb:hover) {
    background-color: hsl(var(--muted-foreground) / 0.5);
  }
</style>