<script lang="ts">
  import Card from '../ui/card.svelte';
  import { FileIcon } from 'lucide-svelte';
</script>

<Card class="p-5 space-y-5">
  <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
    <div class="flex items-start gap-3">
      <div class="w-12 h-12 rounded-md bg-muted animate-pulse flex items-center justify-center">
        <FileIcon class="h-6 w-6 text-muted-foreground" />
      </div>
      <div class="flex-1 space-y-2">
        <div class="h-6 bg-muted rounded w-3/4 animate-pulse"></div>
        <div class="flex items-center gap-2">
          <div class="h-4 bg-muted rounded w-20 animate-pulse"></div>
          <div class="h-4 bg-muted rounded w-16 animate-pulse"></div>
        </div>
      </div>
    </div>

    <div class="flex items-center gap-2 flex-wrap">
      <div class="h-6 bg-muted rounded w-16 animate-pulse"></div>
      <div class="h-6 bg-muted rounded w-20 animate-pulse"></div>
    </div>
  </div>

  <div class="grid gap-4 md:grid-cols-2">
    <!-- Left Column: Technical identifiers skeleton -->
    <div class="space-y-3">
      <div>
        <p class="text-xs uppercase tracking-wide text-muted-foreground mb-2">File hash</p>
        <div class="h-8 bg-muted rounded animate-pulse"></div>
      </div>

      <div>
        <p class="text-xs uppercase tracking-wide text-muted-foreground mb-2">Details</p>
        <div class="space-y-2">
          <div class="flex justify-between items-center">
            <div class="h-4 bg-muted rounded w-20 animate-pulse"></div>
            <div class="h-4 bg-muted rounded w-8 animate-pulse"></div>
          </div>
          <div class="flex justify-between items-center">
            <div class="h-4 bg-muted rounded w-12 animate-pulse"></div>
            <div class="h-4 bg-muted rounded w-16 animate-pulse"></div>
          </div>
          <div class="flex justify-between items-center">
            <div class="h-4 bg-muted rounded w-16 animate-pulse"></div>
            <div class="h-4 bg-muted rounded w-20 animate-pulse"></div>
          </div>
        </div>
      </div>
    </div>

    <!-- Right Column: Available peers skeleton -->
    <div class="space-y-3">
      <p class="text-xs uppercase tracking-wide text-muted-foreground mb-2">Available peers</p>
      <div class="space-y-2 max-h-40 overflow-auto pr-1">
        {#each Array(3) as _}
          <div class="flex items-start gap-2 rounded-md border border-border/50 bg-muted/40 p-2 overflow-hidden animate-pulse">
            <div class="mt-0.5 h-2 w-2 rounded-full bg-gray-300 flex-shrink-0"></div>
            <div class="space-y-1 flex-1">
              <div class="h-4 bg-gray-300 rounded w-3/4"></div>
              <div class="h-3 bg-gray-200 rounded w-1/4"></div>
            </div>
          </div>
        {/each}
      </div>
      <p class="text-xs text-muted-foreground text-center animate-pulse">Loading seeder information...</p>
    </div>
  </div>

  <div class="flex flex-col sm:flex-row gap-3 sm:items-center sm:justify-between">
    <div class="text-xs text-muted-foreground">
      <div class="h-4 bg-muted rounded w-32 animate-pulse"></div>
    </div>
    <div class="flex items-center gap-2">
      <div class="h-9 w-9 bg-muted rounded animate-pulse"></div>
      <div class="h-10 bg-muted rounded w-24 animate-pulse"></div>
    </div>
  </div>
</Card>
