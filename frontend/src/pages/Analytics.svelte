<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { TrendingUp, Upload, DollarSign, HardDrive, Award } from 'lucide-svelte'
  import { files, wallet, networkStats } from '$lib/stores'
  import { onMount } from 'svelte'
  
  let uploadedFiles: any[] = []
  let downloadedFiles: any[] = []
  let totalUploaded = 0
  let totalDownloaded = 0
  let earningsHistory: any[] = []
  let storageUsed = 0
  let bandwidthUsed = { upload: 0, download: 0 }
  
  // Calculate statistics
  $: {
    uploadedFiles = $files.filter(f => f.status === 'uploaded' || f.status === 'seeding')
    downloadedFiles = $files.filter(f => f.status === 'completed')
    totalUploaded = uploadedFiles.reduce((sum, f) => sum + f.size, 0)
    totalDownloaded = downloadedFiles.reduce((sum, f) => sum + f.size, 0)
    storageUsed = totalUploaded + totalDownloaded
  }
  
  // Generate mock earnings history
  onMount(() => {
    const days = 30
    const history = []
    let cumulative = 0
    
    for (let i = days; i >= 0; i--) {
      const date = new Date()
      date.setDate(date.getDate() - i)
      const dailyEarning = Math.random() * 20
      cumulative += dailyEarning
      
      history.push({
        date: date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }),
        earnings: dailyEarning,
        cumulative: cumulative
      })
    }
    
    earningsHistory = history
    
    // Update bandwidth usage periodically
    const interval = setInterval(() => {
      bandwidthUsed = {
        upload: bandwidthUsed.upload + Math.random() * 100,
        download: bandwidthUsed.download + Math.random() * 150
      }
    }, 3000)
    
    return () => clearInterval(interval)
  })
  
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
  
  <!-- Key Metrics -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Total Earnings</p>
          <p class="text-2xl font-bold">{$wallet.totalEarned.toFixed(2)} CN</p>
          <p class="text-xs text-green-600 flex items-center gap-1 mt-1">
            <TrendingUp class="h-3 w-3" />
            +12.5% this week
          </p>
        </div>
        <DollarSign class="h-8 w-8 text-muted-foreground" />
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Storage Used</p>
          <p class="text-2xl font-bold">{formatSize(storageUsed)}</p>
          <Progress value={storageUsed} max={10737418240} class="mt-2 h-1" />
        </div>
        <HardDrive class="h-8 w-8 text-muted-foreground" />
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
        <Upload class="h-8 w-8 text-muted-foreground" />
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Reputation</p>
          <p class="text-2xl font-bold">{$wallet.reputation || 4.5}/5.0</p>
          <div class="flex gap-0.5 mt-1">
            {#each Array(5) as _, i}
              <span class="text-yellow-500">
                {i < Math.floor($wallet.reputation || 4.5) ? '★' : '☆'}
              </span>
            {/each}
          </div>
        </div>
        <Award class="h-8 w-8 text-muted-foreground" />
      </div>
    </Card>
  </div>
  
  <!-- Bandwidth Usage -->
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
  
  <!-- Top Performing Files -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Top Earning Files</h2>
      <div class="space-y-2">
        {#each topEarners as file, i}
          <div class="flex items-center justify-between p-2 rounded hover:bg-secondary">
            <div class="flex items-center gap-2">
              <span class="text-sm font-medium text-muted-foreground">#{i + 1}</span>
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
          <div class="flex items-center justify-between p-2 rounded hover:bg-secondary">
            <div class="flex items-center gap-2">
              <span class="text-sm font-medium text-muted-foreground">#{i + 1}</span>
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
  
  <!-- Earnings Chart (simplified) -->
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">Earnings History (Last 30 Days)</h2>
    <div class="h-48 flex items-end gap-1">
      {#each earningsHistory.slice(-30) as day}
        <div 
          class="flex-1 bg-primary/20 hover:bg-primary/30 transition-colors rounded-t"
          style="height: {(day.earnings / 20) * 100}%"
          title="{day.date}: {day.earnings.toFixed(2)} CN"
        ></div>
      {/each}
    </div>
    <div class="flex justify-between mt-2 text-xs text-muted-foreground">
      <span>{earningsHistory[0]?.date}</span>
      <span>{earningsHistory[earningsHistory.length - 1]?.date}</span>
    </div>
  </Card>
</div>