<script lang="ts">
  import { favorites, type FavoriteFile } from '$lib/stores/favorites';
  import { Star, Download, Trash2, FileIcon } from 'lucide-svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Card from '$lib/components/ui/card.svelte';
  import { toHumanReadableSize } from '$lib/utils';
  import { createEventDispatcher } from 'svelte';
  import { t } from 'svelte-i18n';
  
  const tr = (key: string, params?: any): string => $t(key, params);
  const dispatch = createEventDispatcher<{ download: { hash: string; name: string } }>();

  function removeFavorite(hash: string) {
    favorites.remove(hash);
  }

  function downloadFavorite(fav: FavoriteFile) {
    dispatch('download', { hash: fav.hash, name: fav.name });
  }

  function clearAll() {
    if (confirm(tr('favorites.confirmClear'))) {
      favorites.clear();
    }
  }

  $: sortedFavorites = [...$favorites].sort((a, b) => 
    b.addedAt.getTime() - a.addedAt.getTime()
  );
</script>

{#if $favorites.length === 0}
  <div class="text-center py-12 text-muted-foreground">
    <Star class="h-12 w-12 mx-auto mb-4 opacity-20" />
    <p class="text-lg font-medium mb-2">{tr('favorites.empty.title')}</p>
    <p class="text-sm">{tr('favorites.empty.description')}</p>
  </div>
{:else}
  <div class="space-y-3">
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold flex items-center gap-2">
        <Star class="h-5 w-5 text-yellow-500 fill-current" />
        {tr('favorites.title')} ({$favorites.length})
      </h3>
      <Button variant="outline" size="sm" on:click={clearAll}>
        <Trash2 class="h-3.5 w-3.5 mr-2" />
        {tr('favorites.clearAll')}
      </Button>
    </div>

    {#each sortedFavorites as fav (fav.hash)}
      <Card class="p-4 hover:shadow-md transition-shadow">
        <div class="flex items-center justify-between gap-4">
          <div class="flex items-center gap-3 flex-1 min-w-0">
            <FileIcon class="h-8 w-8 text-blue-500 flex-shrink-0" />
            <div class="flex-1 min-w-0">
              <p class="font-medium truncate">{fav.name}</p>
              <div class="flex items-center gap-3 text-xs text-muted-foreground mt-1">
                <span>{toHumanReadableSize(fav.size)}</span>
                {#if fav.protocol}
                  <span class="uppercase">{fav.protocol}</span>
                {/if}
                {#if fav.seeders !== undefined}
                  <span>{fav.seeders} {tr('favorites.seeders')}</span>
                {/if}
              </div>
            </div>
          </div>

          <div class="flex items-center gap-2 flex-shrink-0">
            <Button
              variant="outline"
              size="icon"
              on:click={() => removeFavorite(fav.hash)}
              class="h-8 w-8"
              title={tr('favorites.remove')}
            >
              <Star class="h-4 w-4 text-yellow-500 fill-current" />
            </Button>
            <Button
              size="sm"
              on:click={() => downloadFavorite(fav)}
              title={tr('favorites.download')}
            >
              <Download class="h-3.5 w-3.5 mr-2" />
              {tr('favorites.download')}
            </Button>
          </div>
        </div>
      </Card>
    {/each}
  </div>
{/if}
