<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { TrendingUp, Upload, DollarSign, HardDrive, Award } from 'lucide-svelte'
  //CHANGING IMPORT TO ADD PROXY NODES
  import { files, wallet, networkStats, proxyNodes } from '$lib/stores'
  //import { files, wallet, networkStats } from '$lib/stores'
  import { onMount } from 'svelte'
  // import { DatePicker } from '@svelte-plugins/datepicker'
  // import { Popover } from "bits-ui"; // Added Popover component

  let uploadedFiles: any[] = []
  let downloadedFiles: any[] = []
  let totalUploaded = 0
  let totalDownloaded = 0
  let earningsHistory: any[] = []
  let storageUsed = 0
  let bandwidthUsed = { upload: 0, download: 0 }
  //ADDING LATENCY STATE AND HELPER 
  // Latency analytics (derived from proxy nodes)
let avgLatency = 0
let p95Latency = 0
let bestLatency = 0
let latencyHistory: { date: string; latency: number }[] = []

function computeLatencyStats() {
  // Use the live values from $proxyNodes
  const latencies = $proxyNodes
    .map(n => n.latency)
    .filter(l => typeof l === 'number' && isFinite(l))

  if (latencies.length === 0) {
    avgLatency = 0
    p95Latency = 0
    bestLatency = 0
    return
  }

  latencies.sort((a, b) => a - b)
  const sum = latencies.reduce((s, v) => s + v, 0)
  avgLatency = sum / latencies.length
  const idx = Math.floor(0.95 * (latencies.length - 1))
  p95Latency = latencies[idx]
  bestLatency = latencies[0]
}

  
  // Dynamic earnings history chart values
  let hoveredDay: typeof earningsHistory[0] | null = null;
  let hoveredIndex: number | null = null;
  let periodPreset: string = '30d';
  // Use a single array for the date range
  let customDateRange: [Date | null, Date | null] = [null, null];
  const periodPresets = [
    { label: 'Last 7 days', value: '7d' },
    { label: 'Last 30 days', value: '30d' },
    { label: 'This Month', value: 'month' },
    { label: 'Last Month', value: 'lastmonth' },
    { label: 'Year to Date', value: 'ytd' },
    // { label: 'Custom…', value: 'custom' }
  ];
  const MAX_BARS = 60;

  // Calculate statistics
  $: {
    uploadedFiles = $files.filter(f => f.status === 'uploaded' || f.status === 'seeding')
    downloadedFiles = $files.filter(f => f.status === 'completed')
    totalUploaded = uploadedFiles.reduce((sum, f) => sum + f.size, 0)
    totalDownloaded = downloadedFiles.reduce((sum, f) => sum + f.size, 0)
    storageUsed = totalUploaded + totalDownloaded
  }
  
  $: chartMax = chartData.length > 0 ? Math.max(...chartData.map(d => d.earnings)) : 1;

  $: filteredHistory = (() => {
    const now = new Date();

    // Helper to format date string
    function formatDate(date: Date) {
      return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
    }

    // Helper to pad missing days
    function padHistory(start: Date, end: Date) {
      const days = [];
      let d = new Date(start);
      while (d <= end) {
        const dateStr = formatDate(d);
        const found = earningsHistory.find(e => e.date === dateStr);
        days.push(found ? found : { date: dateStr, earnings: 0, cumulative: 0 });
        d.setDate(d.getDate() + 1);
      }
      return days;
    }

    if (periodPreset === '7d') {
      const start = new Date(now);
      start.setDate(now.getDate() - 6);
      return padHistory(start, now);
    }
    if (periodPreset === '30d') {
      const start = new Date(now);
      start.setDate(now.getDate() - 29);
      return padHistory(start, now);
    }
    if (periodPreset === 'month') {
      const start = new Date(now.getFullYear(), now.getMonth(), 1);
      const end = new Date(now.getFullYear(), now.getMonth() + 1, 0);
      return padHistory(start, end);
    }
    if (periodPreset === 'lastmonth') {
      const start = new Date(now.getFullYear(), now.getMonth() - 1, 1);
      const end = new Date(now.getFullYear(), now.getMonth(), 0);
      return padHistory(start, end);
    }
    if (periodPreset === 'ytd') {
      const start = new Date(now.getFullYear(), 0, 1);
      return padHistory(start, now);
    }
    if (
      periodPreset === 'custom' &&
      customDateRange[0] &&
      customDateRange[1]
    ) {
      const start = new Date(customDateRange[0]);
      const end = new Date(customDateRange[1]);
      if (start > end) return [];
      return padHistory(start, end);
    }
    return earningsHistory;
  })();

  function handlePresetChange(value: string) {
    periodPreset = value;
  }

  // Generate mock earnings history once on mount
  onMount(() => {
    const days = 365;
    const history = []
    let cumulative = 0
    
    for (let i = days; i >= 0; i--) {
      const date = new Date()
      date.setDate(date.getDate() - i)
      const dailyEarning = Math.random() * 20
      cumulative += dailyEarning

      history.push({
        date: date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' }),
        earnings: dailyEarning,
        cumulative: cumulative
      })
    }
    
    earningsHistory = history
    //ADDING THIS: INITIALIZING THE LATENCY History
    // Generate mock latency history (last 30 points)
    const lhist: { date: string; latency: number }[] = []
    for (let i = 29; i >= 0; i--) {
      const d = new Date()
      d.setDate(d.getDate() - i)
      // Base on current avg and add slight jitter
      const base = $proxyNodes.length
        ? ($proxyNodes.reduce((s, n) => s + (n.latency || 0), 0) / $proxyNodes.length)
        : 80
      const jitter = (Math.random() - 0.5) * 20
      lhist.push({ date: d.toLocaleDateString(), latency: Math.max(5, base + jitter) })
    }
    latencyHistory = lhist
    computeLatencyStats()

    
    // Update bandwidth usage periodically
    /*const interval = setInterval(() => {
      bandwidthUsed = {
        upload: bandwidthUsed.upload + Math.random() * 100,
        download: bandwidthUsed.download + Math.random() * 150
      }
    }, 3000)*/

    //ADDING THIS extending refresh timer to include latency also
    // Update bandwidth & latency periodically
    const interval = setInterval(() => {
      bandwidthUsed = {
        upload: bandwidthUsed.upload + Math.random() * 100,
        download: bandwidthUsed.download + Math.random() * 150
      }

      // Simulate latency jitter and keep short history (30 points)
      const base = $proxyNodes.length
        ? ($proxyNodes.reduce((s, n) => s + (n.latency || 0), 0) / $proxyNodes.length)
        : 80
      const jitter = (Math.random() - 0.5) * 15
      latencyHistory = [
        ...latencyHistory.slice(1),
        { date: new Date().toLocaleTimeString(), latency: Math.max(5, base + jitter) }
      ]
      computeLatencyStats()
    }, 3000)

    
    return () => clearInterval(interval)
  })
  
  // Aggregation function
  function aggregateData(data: any[], maxBars: number) {
    if (data.length <= maxBars) return data;
    const groupSize = Math.ceil(data.length / maxBars);
    const result = [];
    for (let i = 0; i < data.length; i += groupSize) {
      const group = data.slice(i, i + groupSize);
      // Aggregate earnings and use the first date in the group
      const earnings = group.reduce((sum, d) => sum + d.earnings, 0);
      result.push({
        date: group[0].date + (group.length > 1 ? ` – ${group[group.length - 1].date}` : ''),
        earnings,
        cumulative: group[group.length - 1].cumulative
      });
    }
    return result;
  }

  $: chartData = aggregateData(filteredHistory, MAX_BARS);

  function formatSize(bytes: number): string {
    const units = ['B', 'KB', 'MB', 'GB', 'TB']
    let size = bytes
    let unitIndex = 0
    
    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024
      unitIndex++
    }
    
    return `${size.toFixed(2)} ${units[unitIndex]}`
  }
  
  // Calculate top performers
  $: topEarners = uploadedFiles
    .sort((a, b) => ((b.seeders || 0) * (b.size || 0)) - ((a.seeders || 0) * (a.size || 0)))
    .slice(0, 5)

  $: popularFiles = [...$files]
    .sort((a, b) => (b.seeders || 0) - (a.seeders || 0))
    .slice(0, 5)
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Analytics Dashboard</h1>
    <p class="text-muted-foreground mt-2">Track your performance and network activity</p>
  </div>
  
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Total Earnings</p>
          <p class="text-2xl font-bold">{$wallet.totalEarned.toFixed(2)} Chiral</p>
          <p class="text-xs text-green-600 flex items-center gap-1 mt-1">
            <TrendingUp class="h-3 w-3" />
            +12.5% this week
          </p>
        </div>
        <div class="p-2 bg-green-500/10 rounded-lg">
          <DollarSign class="h-5 w-5 text-green-500" />
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Storage Used</p>
          <p class="text-2xl font-bold">{formatSize(storageUsed)}</p>
          <Progress value={storageUsed} max={10737418240} class="mt-2 h-1" />
        </div>
        <div class="p-2 bg-purple-500/10 rounded-lg">
          <HardDrive class="h-5 w-5 text-purple-500" />
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Files Shared</p>
          <p class="text-2xl font-bold">{uploadedFiles.length}</p>
          <p class="text-xs text-muted-foreground mt-1">
            {downloadedFiles.length} downloaded
          </p>
        </div>
        <div class="p-2 bg-blue-500/10 rounded-lg">
          <Upload class="h-5 w-5 text-blue-500" />
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Reputation</p>
          <p class="text-2xl font-bold">{$wallet.reputation || 4.5}/5.0</p>
          <!-- Stars (replaces your existing block) -->
<div
  class="flex gap-0.5 mt-1"
  aria-label={"Reputation " + (($wallet.reputation ?? 4.5).toFixed(1)) + " out of 5"}
>
  {#each Array(5) as _, i}
    <span class="relative inline-block leading-none align-middle" style="width: 1em">
      <!-- empty star -->
      <span class="text-yellow-500 opacity-30 select-none">★</span>

      <!-- filled portion (handles full and partial stars without special glyphs) -->
      <span
        class="absolute inset-0 overflow-hidden"
        style="width: {Math.max(0, Math.min(1, (($wallet.reputation ?? 4.5) - i))) * 100}%"
      >
        <span class="text-yellow-500 select-none">★</span>
      </span>
    </span>
  {/each}
</div>
        </div>
        <div class="p-2 bg-yellow-500/10 rounded-lg">
          <Award class="h-5 w-5 text-yellow-500" />
        </div>
      </div>
    </Card>
  </div>
  
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Bandwidth Usage (Today)</h2>
      <div class="space-y-4">
        <div>
          <div class="flex justify-between mb-2">
            <span class="text-sm">Upload</span>
            <span class="text-sm font-medium">{formatSize(bandwidthUsed.upload * 1048576)}</span>
          </div>
          <Progress value={bandwidthUsed.upload} max={1000} />
        </div>
        <div>
          <div class="flex justify-between mb-2">
            <span class="text-sm">Download</span>
            <span class="text-sm font-medium">{formatSize(bandwidthUsed.download * 1048576)}</span>
          </div>
          <Progress value={bandwidthUsed.download} max={1000} />
        </div>
        <div class="pt-2 border-t">
          <div class="flex justify-between text-sm">
            <span class="text-muted-foreground">Total</span>
            <span class="font-medium">
              {formatSize((bandwidthUsed.upload + bandwidthUsed.download) * 1048576)}
            </span>
          </div>
        </div>
      </div>
    </Card>
    
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Network Activity</h2>
      <div class="space-y-3">
        <div class="flex justify-between items-center">
          <span class="text-sm">Active Uploads</span>
          <Badge>{uploadedFiles.filter(f => f.status === 'seeding').length}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">Active Downloads</span>
          <Badge>{$files.filter(f => f.status === 'downloading').length}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">Queued Downloads</span>
          <Badge variant="outline">{$files.filter(f => f.status === 'queued').length}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">Total Transactions</span>
          <Badge variant="secondary">{$networkStats.totalTransactions}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">Network Files</span>
          <Badge variant="secondary">{$networkStats.totalFiles}</Badge>
        </div>
      </div>
    </Card>
  </div>

  <!-- ADDING THIS -->
  <!-- Network Latency -->
<div class="grid grid-cols-1 md:grid-cols-2 gap-6">
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">Network Latency</h2>
    <div class="grid grid-cols-1 sm:grid-cols-3 gap-6 mb-6">
      <div>
        <p class="text-xs text-muted-foreground mb-1">Average</p>
        <p class="text-2xl font-bold">{avgLatency.toFixed(0)} ms</p>
      </div>
      <div>
        <p class="text-xs text-muted-foreground mb-1">P95</p>
        <p class="text-2xl font-bold">{p95Latency.toFixed(0)} ms</p>
      </div>
      <div>
        <p class="text-xs text-muted-foreground mb-1">Best</p>
        <p class="text-2xl font-bold">{bestLatency.toFixed(0)} ms</p>
      </div>
    </div>

    <div class="space-y-4">
      <div>
        <div class="flex justify-between mb-2">
          <span class="text-sm">Current Avg</span>
          <span class="text-sm font-medium">{avgLatency.toFixed(0)} ms</span>
        </div>
        <Progress value={Math.min(avgLatency, 300)} max={300} />
        <p class="text-xs text-muted-foreground mt-1">
          0–50 ms (great), 50–150 ms (ok), &gt;150 ms (poor)
        </p>
      </div>

      <div class="pt-2 border-t text-sm grid grid-cols-2 gap-2">
        <div class="flex justify-between items-center">
          <span class="text-sm">Nodes Reporting</span>
          <Badge variant="outline">{$proxyNodes.length}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">Sample Size</span>
          <Badge variant="outline">{latencyHistory.length}</Badge>
        </div>
      </div>
    </div>
  </Card>

  <script>
    let hoveredLatency = null;
    let hoveredIndex = null;
  </script>

  <Card class="p-6">
    <h3 class="text-md font-medium mb-4">Latency (recent)</h3>
    <div class="flex h-48 gap-2">
    <!-- Y-axis labels -->
    <div class="flex flex-col justify-between text-xs text-muted-foreground pr-2">
      <span>300 ms</span>
      <span>150 ms</span>
      <span>0</span>
    </div>
    
    <!-- Bars + gridlines -->
    <div class="relative flex-1 flex items-end gap-1">
      <!-- Gridlines -->
      <div class="absolute inset-0 flex flex-col justify-between">
        <div class="border-t border-muted-foreground/20"></div>
        <div class="border-t border-muted-foreground/20"></div>
        <div class="border-t border-muted-foreground/20"></div>
      </div>

      <!-- Bars -->
      {#each latencyHistory as p, i}
        <div
          role="button"
          tabindex="0"
          class="flex-1 relative bg-gradient-to-t from-green-400/30 to-red-500/60 hover:from-green-500/60 hover:to-red-600/90 transition-all rounded-t shadow-sm"
          style="height: {(Math.min(p.latency, 300) / 300) * 100}%"
          aria-label="{p.date}: {p.latency.toFixed(0)} ms"
          on:mouseenter={() => { hoveredLatency = p; hoveredIndex = i; }}
          on:mouseleave={() => { hoveredLatency = null; hoveredIndex = null; }}
        >
          {#if hoveredIndex === i && hoveredLatency}
            <div
              class="absolute left-1/2 -translate-x-1/2 -top-8 z-10 px-2 py-1 rounded bg-primary text-white text-xs shadow-lg pointer-events-none"
              style="white-space:nowrap;"
            >
              {hoveredLatency.date}: {hoveredLatency.latency.toFixed(0)} ms
              <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 
                border-l-6 border-l-transparent border-r-6 border-r-transparent 
                border-t-6 border-t-primary"></span>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </div>
   
    <div class="flex justify-between mt-2 text-xs text-muted-foreground">
      <span>{latencyHistory[0]?.date}</span>
      <span>{latencyHistory[latencyHistory.length - 1]?.date}</span>
    </div>
    <div class="flex gap-4 mt-2 text-xs text-muted-foreground">
    <span>Min: {Math.min(...latencyHistory.map(p => p.latency)).toFixed(0)} ms</span>
    <span>Max: {Math.max(...latencyHistory.map(p => p.latency)).toFixed(0)} ms</span>
  </div>
  </Card>
</div>
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Top Earning Files</h2>
      <div class="space-y-2">
        {#each topEarners as file, i}
          <div
            class="flex items-center justify-between p-2 rounded hover:bg-secondary"
            style="
              {i === 0 ? 'background: linear-gradient(90deg, #FFD70033 0%, #FFFACD33 100%);' : ''}
              {i === 1 ? 'background: linear-gradient(90deg, #C0C0C033 0%, #F5F5F533 100%);' : ''}
              {i === 2 ? 'background: linear-gradient(90deg, #CD7F3233 0%, #FFE4B533 100%);' : ''}
            "
          >
            <div class="flex items-center gap-2">
              <span
                class="text-sm font-medium"
                style="
                  {i === 0 ? 'color: #FFD700;' : ''}
                  {i === 1 ? 'color: #C0C0C0;' : ''}
                  {i === 2 ? 'color: #CD7F32;' : ''}
                "
              >
                #{i + 1}
              </span>
              <div>
                <p class="text-sm font-medium">{file.name}</p>
                <p class="text-xs text-muted-foreground">{file.seeders || 0} seeders</p>
              </div>
            </div>
            <Badge variant="outline">
              {((file.seeders || 0) * (file.size / 1000000)).toFixed(2)} MB
            </Badge>
          </div>
        {/each}
        {#if topEarners.length === 0}
          <p class="text-sm text-muted-foreground text-center py-4">No earnings yet</p>
        {/if}
      </div>
    </Card>
    
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Popular Files</h2>
      <div class="space-y-2">
        {#each popularFiles as file, i}
          <div
            class="flex items-center justify-between p-2 rounded hover:bg-secondary"
            style="
              {i === 0 ? 'background: linear-gradient(90deg, #FFD70033 0%, #FFFACD33 100%);' : ''}
              {i === 1 ? 'background: linear-gradient(90deg, #C0C0C033 0%, #F5F5F533 100%);' : ''}
              {i === 2 ? 'background: linear-gradient(90deg, #CD7F3233 0%, #FFE4B533 100%);' : ''}
            "
          >
            <div class="flex items-center gap-2">
              <span
                class="text-sm font-medium"
                style="
                  {i === 0 ? 'color: #FFD700;' : ''}
                  {i === 1 ? 'color: #C0C0C0;' : ''}
                  {i === 2 ? 'color: #CD7F32;' : ''}
                "
              >
                #{i + 1}
              </span>
              <div>
                <p class="text-sm font-medium">{file.name}</p>
                <p class="text-xs text-muted-foreground">{formatSize(file.size)}</p>
              </div>
            </div>
            <div class="flex items-center gap-2">
              <Badge variant="outline">
                {file.seeders || 0} Seeders
              </Badge>
              {#if file.leechers && file.leechers > 0}
                <Badge variant="secondary">
                  {file.leechers} Leechers
                </Badge>
              {/if}
            </div>
          </div>
        {/each}
        {#if popularFiles.length === 0}
          <p class="text-sm text-muted-foreground text-center py-4">No files yet</p>
        {/if}
      </div>
    </Card>
  </div>
  
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">Earnings History</h2>
    <div class="flex flex-wrap gap-2 mb-4">
      {#each periodPresets as preset}
        <button
          type="button"
          class="px-3 py-1 rounded-full border transition-colors text-sm
            {periodPreset === preset.value ? 'bg-primary text-white' : 'bg-muted text-primary'}"
          on:click={() => handlePresetChange(preset.value)}
          aria-pressed={periodPreset === preset.value}
          tabindex="0"
        >
          {preset.label}
        </button>
      {/each}
    </div>
    
    <!-- <Popover.Root>
      <Popover.Trigger>
        <button
          class="px-3 py-1 rounded-full border transition-colors text-sm {periodPreset === 'custom' ? 'bg-primary text-white' : 'bg-muted text-primary'}"
          on:click={() => {
            periodPreset = 'custom';
            // The popover itself handles showing/hiding
          }}
          aria-pressed={periodPreset === 'custom'}
          tabindex="0"
        >
          Custom…
        </button>
      </Popover.Trigger>
      <Popover.Content class="z-50 p-4">
        <div class="flex flex-col gap-2 items-center">
          <DatePicker 
            mode="range" 
            bind:value={customDateRange} 
            class="min-w-[280px]"
          />
        </div>
      </Popover.Content>
    </Popover.Root> -->

    <!-- Chart with Y-axis -->
    <div class="flex h-48 gap-2">
      <!-- Y-axis labels -->
      <div class="flex flex-col justify-between text-xs text-muted-foreground pr-2">
        <span>{chartMax.toFixed(0)} Chiral</span>
        <span>{(chartMax / 2).toFixed(0)} Chiral</span>
        <span>0</span>
      </div>

      <div class="relative flex-1 flex items-end gap-1">
        <!-- Gridlines -->
        <div class="absolute inset-0 flex flex-col justify-between">
          <div class="border-t border-muted-foreground/20"></div>
          <div class="border-t border-muted-foreground/20"></div>
          <div class="border-t border-muted-foreground/20"></div>
        </div>
        
        <!-- Bars -->
        {#each chartData as day, i}
          <div
            role="button"
            tabindex="0"
            class="flex-1 bg-gradient-to-t from-blue-400/40 to-blue-500/80 hover:from-blue-500/60 hover:to-blue-600/90 transition-all rounded-t-md shadow-sm relative group"
            style="height: {(day.earnings / chartMax) * 100}%"
            title="{day.date}: {day.earnings.toFixed(2)} Chiral"
            on:mouseenter={() => { hoveredDay = day; hoveredIndex = i; }}
            on:mouseleave={() => { hoveredDay = null; hoveredIndex = null; }}
            aria-label="{day.date}: {day.earnings.toFixed(2)} Chiral"
          >
            {#if hoveredIndex === i && hoveredDay}
              <div
                class="absolute left-1/2 -translate-x-1/2 -top-8 z-10 px-2 py-1 rounded bg-primary text-white text-xs shadow-lg pointer-events-none"
                style="white-space:nowrap;"
              >
                {hoveredDay.date}: {hoveredDay.earnings.toFixed(2)} Chiral
                <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 border-l-6 border-l-transparent border-r-6 border-r-transparent border-t-6 border-t-primary"></span>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </div>

    <!-- X-axis labels -->
    <div class="flex justify-between mt-2 text-xs text-muted-foreground">
      <span>{filteredHistory[0]?.date}</span>
      <span>{filteredHistory[filteredHistory.length - 1]?.date}</span>
    </div>
  </Card>
</div>