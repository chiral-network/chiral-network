<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { peerGeoDistribution } from '$lib/stores';
  import { GEO_REGIONS, projectToMap, UNKNOWN_REGION_ID } from '$lib/geo';
  import type { PeerGeoRegionStat, PeerGeoDistribution } from '$lib/stores';
  import type { GeoRegionConfig } from '$lib/geo';
  import { MapPin, Globe2 } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import { get } from 'svelte/store';

  type TranslateFn = (key: string, params?: Record<string, unknown>) => string;

  const getTranslator = (): TranslateFn => get(t) as TranslateFn;

  const tr = (key: string, params?: Record<string, unknown>) => getTranslator()(key, params);

  const MAP_WIDTH = 360;
  const MAP_HEIGHT = 180;

  let distribution: PeerGeoDistribution = {
    totalPeers: 0,
    regions: [],
    dominantRegionId: null,
    generatedAt: Date.now(),
  };
  let knownRegions: PeerGeoRegionStat[] = [];
  let unknownBucket: PeerGeoRegionStat | undefined;
  let maxCount = 0;
  let hasPeers = false;

  $: distribution = $peerGeoDistribution;
  $: knownRegions = distribution.regions.filter((region) => region.regionId !== UNKNOWN_REGION_ID);
  $: unknownBucket = distribution.regions.find((region) => region.regionId === UNKNOWN_REGION_ID);
  $: maxCount = knownRegions.reduce((max, region) => Math.max(max, region.count), 0);
  $: hasPeers = distribution.totalPeers > 0;

  function markerRadius(count: number): number {
    if (count <= 0 || maxCount === 0) {
      return 0;
    }
    const minRadius = 6;
    const extra = (count / maxCount) * 18;
    return Math.round((minRadius + extra) * 10) / 10;
  }

  type MapRegionMarker = {
    config: GeoRegionConfig;
    stat?: PeerGeoRegionStat;
    x: number;
    y: number;
    radius: number;
  };

  let mapRegions: MapRegionMarker[] = [];

  $: mapRegions = GEO_REGIONS.filter((config) => config.id !== UNKNOWN_REGION_ID).map((config) => {
    const stat = distribution.regions.find((region) => region.regionId === config.id);
    const { x, y } = projectToMap(config.lat, config.lng, MAP_WIDTH, MAP_HEIGHT);
    return {
      config,
      stat,
      x,
      y,
      radius: markerRadius(stat?.count ?? 0),
    };
  });
</script>

<Card class="p-6 space-y-6">
  <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
    <div>
      <h2 class="text-lg font-semibold">{tr('network.geoDistribution.title')}</h2>
      <p class="text-sm text-muted-foreground">{tr('network.geoDistribution.subtitle', { values: { peers: distribution.totalPeers } })}</p>
    </div>
    {#if distribution.dominantRegionId}
      <Badge variant="outline" class="flex items-center gap-2">
        <MapPin class="h-4 w-4 text-primary" />
        {tr('network.geoDistribution.topRegion', {
          values: {
            region: knownRegions.find((region) => region.regionId === distribution.dominantRegionId)?.label ?? distribution.dominantRegionId,
          }
        })}
      </Badge>
    {:else}
      <Badge variant="outline" class="flex items-center gap-2">
        <Globe2 class="h-4 w-4 text-muted-foreground" />
        {tr('network.geoDistribution.noData')}
      </Badge>
    {/if}
  </div>

  <div class="grid gap-6 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
    <div class="bg-muted/60 border border-border/50 rounded-xl overflow-hidden shadow-inner">
      <div class="relative">
        <svg viewBox="0 0 {MAP_WIDTH} {MAP_HEIGHT}" class="w-full h-auto" preserveAspectRatio="xMidYMid meet">
          <defs>
            <radialGradient id="geo-glow" cx="50%" cy="50%" r="50%">
              <stop offset="0%" stop-color="rgba(59,130,246,0.18)" />
              <stop offset="70%" stop-color="rgba(59,130,246,0.08)" />
              <stop offset="100%" stop-color="transparent" />
            </radialGradient>
          </defs>
          <rect width="100%" height="100%" fill="url(#geo-glow)" />
          <g stroke="rgba(148, 163, 184, 0.4)" stroke-width="0.5">
            {#each Array(7) as _, index}
              <line x1="0" y1="{(index + 1) * (MAP_HEIGHT / 8)}" x2="{MAP_WIDTH}" y2="{(index + 1) * (MAP_HEIGHT / 8)}" />
            {/each}
            {#each Array(11) as _, index}
              <line x1="{(index + 1) * (MAP_WIDTH / 12)}" y1="0" x2="{(index + 1) * (MAP_WIDTH / 12)}" y2="{MAP_HEIGHT}" />
            {/each}
          </g>

          {#each mapRegions as marker (marker.config.id)}
            <g>
              {#if marker.radius > 0}
                <circle
                  cx="{marker.x}"
                  cy="{marker.y}"
                  r="{marker.radius}"
                  fill="{marker.config.color}"
                  fill-opacity="0.45"
                />
              {/if}
              <circle
                cx="{marker.x}"
                cy="{marker.y}"
                r="{marker.radius > 0 ? Math.max(3, marker.radius / 3) : 3}"
                fill="{marker.config.color}"
                stroke="rgba(15, 23, 42, 0.6)"
                stroke-width="0.8"
              >
                <title>
                  {tr('network.geoDistribution.tooltip', {
                    values: {
                      region: marker.config.label,
                      count: marker.stat?.count ?? 0,
                    }
                  })}
                </title>
              </circle>
            </g>
          {/each}
        </svg>
      </div>
    </div>

    <div class="space-y-4">
      {#if !hasPeers}
        <div class="p-4 bg-muted border border-dashed border-border rounded-lg text-sm text-muted-foreground">
          {tr('network.geoDistribution.emptyState')}
        </div>
      {:else}
        {#each distribution.regions as region (region.regionId)}
          <div class="p-3 rounded-lg border border-border/60 bg-background/70 shadow-sm">
            <div class="flex items-center justify-between gap-3">
              <div class="flex items-center gap-2 min-w-0">
                <span class="w-2 h-8 rounded-full" style="background-color: {region.color};"></span>
                <div>
                  <p class="font-medium leading-tight">{region.label}</p>
                  <p class="text-xs text-muted-foreground">{region.count} · {region.percentage.toFixed(1)}%</p>
                </div>
              </div>
              <Badge variant="secondary" class="text-xs">{tr('network.geoDistribution.peersShort', { values: { count: region.count } })}</Badge>
            </div>
            <div class="mt-2 h-2 bg-muted rounded-full overflow-hidden">
              <div
                class="h-full rounded-full transition-all"
                style="width: {hasPeers && region.count > 0 ? Math.max(4, region.percentage) : 0}%; background-color: {region.color};"
              ></div>
            </div>
          </div>
        {/each}
      {/if}
      {#if unknownBucket && unknownBucket.count > 0 && hasPeers}
        <p class="text-xs text-muted-foreground">
          {tr('network.geoDistribution.unknownNotice', {
            values: { count: unknownBucket.count }
          })}
        </p>
      {/if}
    </div>
  </div>
</Card>
