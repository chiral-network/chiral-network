<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { TrendingUp, Upload, DollarSign, HardDrive, Award, BarChart3, TrendingUp as LineChart } from 'lucide-svelte'
  import { files, wallet, networkStats } from '$lib/stores';
  import { proxyNodes } from '$lib/proxy';
  import { onMount } from 'svelte'
  import { t } from 'svelte-i18n'
  import { suspiciousActivity } from '$lib/stores'; // only import
  
  let uploadedFiles: any[] = []
  let downloadedFiles: any[] = []
  let totalUploaded = 0
  let totalDownloaded = 0
  // let earningsHistory: any[] = []
  let storageUsed = 0
  let bandwidthUsed = { upload: 0, download: 0 }
  
  // Latency analytics (derived from proxy nodes)
  let avgLatency = 0
  let p95Latency = 0
  let bestLatency = 0
  let latencyHistory: { date: string; latency: number }[] = []
  let hoveredLatency: { date: string; latency: number } | null = null
  let hoveredLatencyIndex: number | null = null

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

  type Earning = {
    date: string;
    earnings: number;
    cumulative: number;
  };

  // Dynamic earnings history chart values
  let hoveredDay: Earning | null = null;
  let hoveredIndex: number | null = null;
  let selectedDay: Earning | null = null;
  let selectedIndex: number | null = null;
  let lastChartSignature: string | null = null;
  let periodPreset: string = '30d';
  let startDateInput = '';
  let endDateInput = '';
  // Chart type toggle - NEW
  let chartType: 'bar' | 'line' = 'bar';
  // Use a single array for the date range
  let customDateRange: [Date | null, Date | null] = [null, null];

  function clearSelection() {
    selectedDay = null;
    selectedIndex = null;
  }

  function formatDateForInput(date: Date): string {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
  }

  $: periodPresets = [
    { label: $t('analytics.periods.7d'), value: '7d' },
    { label: $t('analytics.periods.30d'), value: '30d' },
    { label: $t('analytics.periods.thisMonth'), value: 'month' },
    { label: $t('analytics.periods.lastMonth'), value: 'lastmonth' },
    { label: $t('analytics.periods.ytd'), value: 'ytd' },
    { label: $t('analytics.periods.custom'), value: 'custom' }
  ];
  const MAX_BARS = 60;
  $: {
    if (periodPreset === 'custom') {
      if (startDateInput && endDateInput) {
        // Create dates based on local timezone midnight to prevent off-by-one errors
        const parsedStart = new Date(startDateInput + 'T00:00:00');
        const parsedEnd = new Date(endDateInput + 'T00:00:00');

        if (!isNaN(parsedStart.getTime()) && !isNaN(parsedEnd.getTime())) {
          if (parsedStart <= parsedEnd) {
            customDateRange = [parsedStart, parsedEnd];
          } else {
            customDateRange = [parsedEnd, parsedStart];
            const normalizedStart = formatDateForInput(parsedEnd);
            const normalizedEnd = formatDateForInput(parsedStart);

            if (startDateInput !== normalizedStart) {
              startDateInput = normalizedStart;
            }
            if (endDateInput !== normalizedEnd) {
              endDateInput = normalizedEnd;
            }
          }
        } else {
          customDateRange = [null, null];
        }
      } else {
        customDateRange = [null, null];
      }
    }
  }

  // Function for dynamic data generation
  function generateMockHistory(start: Date, end: Date) {
    const history = [];
    let cumulative = 0; // We can simulate a running total if needed
    let d = new Date(start);

    // Use the start date to create a deterministic "random" seed
    const seed = start.getTime() / 100000;
    
    while (d <= end) {
      // Use a sine wave based on the day of the year for seasonal variation
      const dayOfYear = (d.getTime() - new Date(d.getFullYear(), 0, 0).getTime()) / 86400000;
      const seasonalFactor = (Math.sin((dayOfYear / 365.25) * 2 * Math.PI) + 1.2); //
      
      // Create a pseudo-random value based on the date for daily jitter
      const dailyJitter = Math.sin(d.getTime() / 100000 + seed) * 10 - 5;
      
      const dailyEarning = Math.max(0, 5 * seasonalFactor + dailyJitter);
      cumulative += dailyEarning;

      history.push({
        date: d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' }),
        earnings: dailyEarning,
        cumulative: cumulative
      });
      d.setDate(d.getDate() + 1);
    }
    return history;
  }

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
    let start: Date | null = null;
    let end: Date | null = new Date(now);

    if (periodPreset === '7d') {
      start = new Date(now);
      start.setDate(now.getDate() - 6);
    } else if (periodPreset === '30d') {
      start = new Date(now);
      start.setDate(now.getDate() - 29);
    } else if (periodPreset === 'month') {
      start = new Date(now.getFullYear(), now.getMonth(), 1);
      end = new Date(now.getFullYear(), now.getMonth() + 1, 0);
    } else if (periodPreset === 'lastmonth') {
      start = new Date(now.getFullYear(), now.getMonth() - 1, 1);
      end = new Date(now.getFullYear(), now.getMonth(), 0);
    } else if (periodPreset === 'ytd') {
      start = new Date(now.getFullYear(), 0, 1);
    } else if (periodPreset === 'custom' && customDateRange[0] && customDateRange[1]) {
      start = customDateRange[0];
      end = customDateRange[1];
    }

    if (start && end && start <= end) {
      // Generate data dynamically for the calculated range
      return generateMockHistory(start, end);
    }
    
    // Return empty array if the range is invalid or not set
    return [];
  })();

  function handlePresetChange(value: string) {
    periodPreset = value;
    clearSelection();
    hoveredDay = null;
    hoveredIndex = null;

    if (value === 'custom') {
      const fallbackEnd = endDateInput
        ? new Date(endDateInput + 'T00:00:00')
        : new Date();

      if (!endDateInput || isNaN(fallbackEnd.getTime())) {
        const endForInput = isNaN(fallbackEnd.getTime()) ? new Date() : fallbackEnd;
        endDateInput = formatDateForInput(endForInput);
      }

      if (!startDateInput) {
        const baseEnd = endDateInput
          ? new Date(endDateInput + 'T00:00:00')
          : new Date();
        if (!isNaN(baseEnd.getTime())) {
          const defaultStart = new Date(baseEnd);
          defaultStart.setDate(baseEnd.getDate() - 6);
          startDateInput = formatDateForInput(defaultStart);
        }
      }

      if (startDateInput && endDateInput) {
        const parsedStart = new Date(startDateInput + 'T00:00:00');
        const parsedEnd = new Date(endDateInput + 'T00:00:00');

        if (!isNaN(parsedStart.getTime()) && !isNaN(parsedEnd.getTime()) && parsedStart > parsedEnd) {
          startDateInput = formatDateForInput(parsedEnd);
          endDateInput = formatDateForInput(parsedStart);
        }
      }
    }
  }

  // Generate mock latency history once on mount
  onMount(() => {   
    suspiciousActivity.set([
        { type: 'Unusual Upload', description: 'File > 1GB uploaded unusually fast', date: new Date().toLocaleString(), severity: 'high' },
        { type: 'Multiple Logins', description: 'User logged in from different countries in 5 mins', date: new Date().toLocaleString(), severity: 'medium' },
        { type: 'Failed Downloads', description: 'Several failed download attempts detected', date: new Date().toLocaleString(), severity: 'low' },
      ]);

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

  $: {
    const nextSignature = chartData.map((entry) => `${entry.date}|${entry.earnings}|${entry.cumulative}`).join('::');
    if (lastChartSignature !== null && nextSignature !== lastChartSignature) {
      clearSelection();
      hoveredDay = null;
      hoveredIndex = null;
    }
    lastChartSignature = nextSignature;
  }

  $: {
    if (selectedIndex !== null) {
      if (chartData[selectedIndex]) {
        selectedDay = chartData[selectedIndex];
      } else {
        selectedDay = null;
        selectedIndex = null;
      }
    } else {
      selectedDay = null;
    }
  }

  function selectDay(day: Earning, index: number) {
    if (selectedIndex === index) {
      clearSelection();
    } else {
      selectedDay = day;
      selectedIndex = index;
    }
  }

  function handleKeySelection(event: KeyboardEvent, day: Earning, index: number) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      selectDay(day, index);
    }
  }

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
  
  // Generate SVG path for line chart
  function generateLinePath(data: any[], maxValue: number, width: number, height: number): string {
    if (data.length === 0) return '';
    if (data.length === 1) {
      // Single point - just draw a horizontal line
      const y = height - (data[0].earnings / maxValue) * height;
      return `M 0,${y} L ${width},${y}`;
    }

    const points = data.map((d, i) => {
      const x = (i / (data.length - 1)) * width;
      const y = height - (d.earnings / maxValue) * height;
      return `${x},${y}`;
    });

    return `M ${points.join(' L ')}`;
  }

</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('analytics.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('analytics.subtitle')}</p>
  </div>

  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">{$t('analytics.totalEarnings')}</p>
          <p class="text-2xl font-bold">{$wallet.totalEarned.toFixed(2)} Chiral</p>
          <p class="text-xs text-green-600 flex items-center gap-1 mt-1">
            <TrendingUp class="h-3 w-3" />
            {$t('analytics.earningsThisWeek')}
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
          <p class="text-sm text-muted-foreground">{$t('analytics.storageUsed')}</p>
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
          <p class="text-sm text-muted-foreground">{$t('analytics.filesShared')}</p>
          <p class="text-2xl font-bold">{uploadedFiles.length}</p>
          <p class="text-xs text-muted-foreground mt-1">
            {downloadedFiles.length} {$t('analytics.downloaded')}
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
          <p class="text-sm text-muted-foreground">{$t('analytics.reputation')}</p>
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
      <h2 class="text-lg font-semibold mb-4">{$t('analytics.bandwidthUsage')}</h2>
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <div class="bg-blue-50 rounded-lg p-4 flex flex-col items-center">
        <span class="text-sm text-muted-foreground mb-1">{$t('analytics.upload')}</span>
        <span class="text-2xl font-bold text-blue-600">{formatSize(bandwidthUsed.upload * 1048576)}</span>
      </div>
      <div class="bg-green-50 rounded-lg p-4 flex flex-col items-center">
        <span class="text-sm text-muted-foreground mb-1">{$t('analytics.download')}</span>
        <span class="text-2xl font-bold text-green-600">{formatSize(bandwidthUsed.download * 1048576)}</span>
      </div>
      </div>
      <div class="pt-4 border-t mt-4 flex items-center justify-between text-sm">
        <span class="text-muted-foreground flex items-center gap-1">
          <BarChart3 class="h-4 w-4 text-blue-500" />
          {$t('analytics.totalBandwidthUsed')}
        </span>
        <span class="font-bold text-blue-700 text-lg flex items-center gap-1">
          {formatSize((bandwidthUsed.upload + bandwidthUsed.download) * 1048576)}
          <span class="ml-1 px-2 py-0.5 rounded-full bg-blue-100 text-xs text-blue-600 font-semibold">
        {((bandwidthUsed.upload + bandwidthUsed.download) > 1024 ? 'GB' : 'MB')}
          </span>
        </span>
      </div>
    </Card>

    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">{$t('analytics.networkActivity')}</h2>
      <div class="space-y-3">
        <div class="flex justify-between items-center">
          <span class="text-sm">{$t('analytics.activeUploads')}</span>
          <Badge>{uploadedFiles.filter(f => f.status === 'seeding').length}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">{$t('analytics.activeDownloads')}</span>
          <Badge>{$files.filter(f => f.status === 'downloading').length}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">{$t('analytics.queuedDownloads')}</span>
          <Badge variant="outline">{$files.filter(f => f.status === 'queued').length}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">{$t('analytics.totalTransactions')}</span>
          <Badge variant="secondary">{$networkStats.totalTransactions}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm">{$t('analytics.networkFiles')}</span>
          <Badge variant="secondary">{$networkStats.totalFiles}</Badge>
        </div>
      </div>
    </Card>
  </div>

  <!-- ADDING THIS -->
  <!-- Network Latency -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">{$t('analytics.networkLatency')}</h2>
      <div class="grid grid-cols-1 sm:grid-cols-3 gap-6 mb-6">
        <div>
          <p class="text-xs text-muted-foreground mb-1">{$t('analytics.average')}</p>
          <p class="text-2xl font-bold">{avgLatency.toFixed(0)} ms</p>
        </div>
        <div>
          <p class="text-xs text-muted-foreground mb-1">{$t('analytics.p95')}</p>
          <p class="text-2xl font-bold">{p95Latency.toFixed(0)} ms</p>
        </div>
        <div>
          <p class="text-xs text-muted-foreground mb-1">{$t('analytics.best')}</p>
          <p class="text-2xl font-bold">{bestLatency.toFixed(0)} ms</p>
        </div>
      </div>

      <div class="space-y-4">
        <div>
          <div class="flex justify-between mb-2">
            <span class="text-sm">{$t('analytics.currentAvg')}</span>
            <span class="text-sm font-medium">{avgLatency.toFixed(0)} ms</span>
          </div>
          <Progress
            value={Math.min(avgLatency, 300)}
            max={300}
            indicatorClass="bg-gradient-to-r from-emerald-400 via-yellow-400 to-red-500"
          />
          <p class="text-xs text-muted-foreground mt-1">
            {$t('analytics.latencyHint')}
          </p>
        </div>

        <div class="pt-2 border-t text-sm grid grid-cols-2 gap-2">
          <div class="flex justify-between items-center">
            <span class="text-sm">{$t('analytics.nodesReporting')}</span>
            <Badge variant="outline">{$proxyNodes.length}</Badge>
          </div>
          <div class="flex justify-between items-center">
            <span class="text-sm">{$t('analytics.sampleSize')}</span>
            <Badge variant="outline">{latencyHistory.length}</Badge>
          </div>
        </div>
      </div>
    </Card>

    <Card class="p-6">
      <h3 class="text-lg font-semibold mb-4">{$t('analytics.latencyRecent')}</h3> 
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
                    class="flex-1 bg-gradient-to-t from-blue-400/40 to-blue-500/80 hover:from-blue-500/60 hover:to-blue-600/90 transition-all rounded-t-md shadow-sm relative"
                    style="height: {(Math.min(p.latency, 300) / 300) * 100}%"
                    aria-label="{p.date}: {p.latency.toFixed(0)} ms"
                    on:mouseenter={() => { hoveredLatency = p; hoveredLatencyIndex = i; }}
                    on:mouseleave={() => { hoveredLatency = null; hoveredLatencyIndex = null; }}
            >
              {#if hoveredLatencyIndex === i && hoveredLatency}
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
      <h2 class="text-lg font-semibold mb-4">{$t('analytics.topEarningFiles')}</h2>
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
                <p class="text-xs text-muted-foreground">{file.seeders || 0} {$t('analytics.seeders')}</p>
              </div>
            </div>
            <Badge variant="outline">
              {((file.seeders || 0) * (file.size / 1000000)).toFixed(2)} MB
            </Badge>
          </div>
        {/each}
        {#if topEarners.length === 0}
          <p class="text-sm text-muted-foreground text-center py-4">{$t('analytics.noEarningsYet')}</p>
        {/if}
      </div>
    </Card>

    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">{$t('analytics.popularFiles')}</h2>
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
                {file.seeders || 0} {$t('analytics.seeders')}
              </Badge>
              {#if file.leechers && file.leechers > 0}
                <Badge variant="secondary">
                  {file.leechers} {$t('analytics.leechers')}
                </Badge>
              {/if}
            </div>
          </div>
        {/each}
        {#if popularFiles.length === 0}
          <p class="text-sm text-muted-foreground text-center py-4">{$t('analytics.noFilesYet')}</p>
        {/if}
      </div>
    </Card>
  </div>

  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">{$t('analytics.earningsHistory')}</h2>
      <!-- NEW: Chart type toggle buttons -->
      <div class="flex gap-1 p-1 bg-muted rounded-md">
        <button
                type="button"
                class="flex items-center gap-1 px-2 py-1 rounded text-xs transition-colors
            {chartType === 'bar' ? 'bg-white shadow text-primary' : 'text-muted-foreground hover:text-primary'}"
                on:click={() => chartType = 'bar'}
                aria-pressed={chartType === 'bar'}
        >
          <BarChart3 class="h-3 w-3" />
          {$t('analytics.bars')}
        </button>
        <button
                type="button"
                class="flex items-center gap-1 px-2 py-1 rounded text-xs transition-colors
            {chartType === 'line' ? 'bg-white shadow text-primary' : 'text-muted-foreground hover:text-primary'}"
                on:click={() => chartType = 'line'}
                aria-pressed={chartType === 'line'}
        >
          <LineChart class="h-3 w-3" />
          {$t('analytics.line')}
        </button>
      </div>
    </div>

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

    {#if periodPreset === 'custom'}
      <div class="flex items-center gap-2 mb-4 p-2 bg-muted rounded-md">
        <label for="start-date" class="text-sm text-muted-foreground">{$t('analytics.from')}</label>
        <input
          type="date"
          id="start-date"
          bind:value={startDateInput}
          class="bg-background border rounded px-2 py-1 text-sm"
        />
        <label for="end-date" class="text-sm text-muted-foreground">{$t('analytics.to')}</label>
        <input
          type="date"
          id="end-date"
          bind:value={endDateInput}
          class="bg-background border rounded px-2 py-1 text-sm"
        />
      </div>
    {/if}

    <!-- Chart with Y-axis -->
    <div class="flex h-48 gap-2">
      <!-- Y-axis labels -->
      <div class="flex flex-col justify-between text-xs text-muted-foreground pr-2">
        <span>{chartMax.toFixed(0)} Chiral</span>
        <span>{(chartMax / 2).toFixed(0)} Chiral</span>
        <span>0</span>
      </div>

      <div class="relative flex-1">
        <!-- Gridlines -->
        <div class="absolute inset-0 flex flex-col justify-between">
          <div class="border-t border-muted-foreground/20"></div>
          <div class="border-t border-muted-foreground/20"></div>
          <div class="border-t border-muted-foreground/20"></div>
        </div>

        {#if chartType === 'bar'}
          <!-- Bar Chart -->
          <div class="flex items-end gap-1 h-full">
            {#each chartData as day, i}
              <div
                      role="button"
                      tabindex="0"
                      class="flex-1 bg-gradient-to-t from-blue-400/40 to-blue-500/80 hover:from-blue-500/60 hover:to-blue-600/90 transition-all rounded-t-md shadow-sm relative group"
                      style="height: {(day.earnings / chartMax) * 100}%"
                      title="{day.date}: {day.earnings.toFixed(2)} Chiral"
                      on:mouseenter={() => { hoveredDay = day; hoveredIndex = i; }}
                      on:mouseleave={() => { hoveredDay = null; hoveredIndex = null; }}
                      on:click={() => selectDay(day, i)}
                      on:keydown={(event) => handleKeySelection(event, day, i)}
                      aria-pressed={selectedIndex === i}
                      aria-label="{day.date}: {day.earnings.toFixed(2)} Chiral"
              >
                {#if selectedIndex === i && selectedDay}
                  <div
                          class="absolute left-1/2 -translate-x-1/2 -top-8 z-10 px-2 py-1 rounded bg-primary text-white text-xs shadow-lg pointer-events-none"
                          style="white-space:nowrap;"
                  >
                    {selectedDay.date}: {selectedDay.earnings.toFixed(2)} Chiral
                    <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 border-l-6 border-l-transparent border-r-6 border-r-transparent border-t-6 border-t-primary"></span>
                  </div>
                {/if}
                {#if hoveredIndex === i && hoveredDay && hoveredIndex !== selectedIndex}
                  <div
                          class="absolute left-1/2 -translate-x-1/2 -top-8 z-20 px-2 py-1 rounded bg-muted-foreground text-background text-xs shadow-lg pointer-events-none"
                          style="white-space:nowrap;"
                  >
                    {hoveredDay.date}: {hoveredDay.earnings.toFixed(2)} Chiral
                    <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 border-l-6 border-l-transparent border-r-6 border-r-transparent border-t-6 border-t-muted-foreground"></span>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {:else}
          <!-- Line Chart -->
          <div class="relative h-full">
            <svg class="w-full h-full" viewBox="0 0 100 100" preserveAspectRatio="none">
              <!-- Line -->
              <path
                      d={generateLinePath(chartData, chartMax, 100, 100)}
                      fill="none"
                      stroke="rgb(0, 0, 0)"
                      stroke-width="0.5"
                      class="drop-shadow-sm"
              />
              <!-- Area under the line -->
              <path
                      d="{generateLinePath(chartData, chartMax, 100, 100)} L 100,100 L 0,100 Z"
                      fill="url(#gradient)"
                      opacity="0.3"
              />
              <!-- Data points -->
              {#each chartData as day, i}
                <circle
                        cx={i / (chartData.length - 1) * 100}
                        cy={100 - (day.earnings / chartMax) * 100}
                        r="0.8"
                        fill="rgb(0, 0, 0)"
                        stroke="white"
                        stroke-width="0.2"
                        class="cursor-pointer hover:r-1.2 transition-all"
                        role="button"
                        tabindex="0"
                        aria-label="Data point for {day.date}: {day.earnings} earnings"
                        on:mouseenter={() => { hoveredDay = day; hoveredIndex = i; }}
                        on:mouseleave={() => { hoveredDay = null; hoveredIndex = null; }}
                        on:click={() => selectDay(day, i)}
                        on:keydown={(event) => handleKeySelection(event, day, i)}
                        aria-pressed={selectedIndex === i}
                />
              {/each}

              <!-- Gradient definition -->
              <defs>
                <linearGradient id="gradient" x1="0%" y1="0%" x2="0%" y2="100%">
                  <stop offset="0%" style="stop-color:rgb(59, 130, 246);stop-opacity:0.4" />
                  <stop offset="100%" style="stop-color:rgb(59, 130, 246);stop-opacity:0.05" />
                </linearGradient>
              </defs>
            </svg>

            <!-- Tooltips for line chart -->
            {#if hoveredDay && hoveredIndex !== null && hoveredIndex !== selectedIndex}
              <div
                      class="absolute z-20 px-2 py-1 rounded bg-muted-foreground text-background text-xs shadow-lg pointer-events-none"
                      style="
                  left: {chartData.length > 1 ? (hoveredIndex / (chartData.length - 1)) * 100 : 50}%;
                  top: {100 - (hoveredDay.earnings / chartMax) * 100}%;
                  transform: translate(-50%, -100%);
                  margin-top: -8px;
                  white-space: nowrap;
                "
              >
                {hoveredDay.date}: {hoveredDay.earnings.toFixed(2)} Chiral
                <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 border-l-6 border-l-transparent border-r-6 border-r-transparent border-t-6 border-t-muted-foreground"></span>
              </div>
            {/if}

            {#if selectedDay && selectedIndex !== null}
              <div
                      class="absolute z-10 px-2 py-1 rounded bg-primary text-white text-xs shadow-lg pointer-events-none"
                      style="
                  left: {chartData.length > 1 ? (selectedIndex / (chartData.length - 1)) * 100 : 50}%;
                  top: {100 - (selectedDay.earnings / chartMax) * 100}%;
                  transform: translate(-50%, -100%);
                  margin-top: -8px;
                  white-space: nowrap;
                "
              >
                {selectedDay.date}: {selectedDay.earnings.toFixed(2)} Chiral
                <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 border-l-6 border-l-transparent border-r-6 border-r-transparent border-t-6 border-t-primary"></span>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- X-axis labels -->
    <div class="flex justify-between mt-2 text-xs text-muted-foreground">
      <span>{filteredHistory[0]?.date}</span>
      <span>{filteredHistory[filteredHistory.length - 1]?.date}</span>
    </div>
  </Card>

  <Card class="p-6">
  <h2 class="text-lg font-semibold mb-4">{$t('analytics.suspiciousActivity')}</h2>
  {#if $suspiciousActivity.length > 0}
    <div class="space-y-2">
      {#each $suspiciousActivity as alert}
        <div class="flex items-center justify-between p-2 rounded hover:bg-red-50 transition">
          <div>
            <p class="text-sm font-medium">{alert.type}</p>
            <p class="text-xs text-muted-foreground">{alert.description}</p>
            <p class="text-xs text-muted-foreground mt-1">{alert.date}</p>
          </div>
          <span
            class="px-2 py-0.5 rounded text-xs font-semibold"
            class:red-500={alert.severity === 'high'}
            class:yellow-500={alert.severity === 'medium'}
            class:green-500={alert.severity === 'low'}
            style="background-color: {alert.severity === 'high' ? '#fee2e2' : alert.severity === 'medium' ? '#fef3c7' : '#dcfce7'}"
          >
            {alert.severity.toUpperCase()}
          </span>
        </div>
      {/each}
    </div>
  {:else}
    <p class="text-sm text-muted-foreground text-center py-4">{$t('analytics.noSuspiciousActivity')}</p>
  {/if}
</Card>

</div>
