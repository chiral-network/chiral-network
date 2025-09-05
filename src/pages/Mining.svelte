<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { Cpu, Zap, TrendingUp, Award, Play, Pause, Settings, Hash, Clock, Coins, ChevronsUpDown } from 'lucide-svelte'
  import { onMount, onDestroy } from 'svelte'
  
  // Mining state
  let isMining = false
  let hashRate = 0
  let totalHashes = 0
  let blocksFound = 0
  let totalRewards = 0
  let currentDifficulty = 4
  let miningPool = 'solo'
  let cpuThreads = navigator.hardwareConcurrency || 4
  let selectedThreads = Math.floor(cpuThreads / 2)
  let miningIntensity = 50 // percentage
  
  // Statistics
  let sessionStartTime = Date.now()
  let lastBlockTime = 0
  let estimatedTimeToBlock = 0
  let powerConsumption = 0
  let efficiency = 0
  let temperature = 45

  // Uptime tick (forces template to re-render every second while mining)
  let uptimeNow: number = Date.now()
  let uptimeInterval: number | null = null
  
  // Mining history
  let miningHistory = []
  let recentBlocks = []
  
  // Mock mining intervals
  let miningInterval: number | null = null
  let statsInterval: number | null = null
  
  function startMining() {
    isMining = true
    sessionStartTime = Date.now()

    // start uptime ticker so UI updates every second
    uptimeNow = Date.now()
    if (!uptimeInterval) {
      uptimeInterval = setInterval(() => {
        uptimeNow = Date.now()
      }, 1000) as unknown as number
    }
    
    // Simulate mining
    miningInterval = setInterval(() => {
      if (!isMining) return
      
      // Update hash rate based on threads and intensity
      const baseHashRate = 50 // H/s per thread
      hashRate = baseHashRate * selectedThreads * (miningIntensity / 100) + (Math.random() * 10 - 5)
      totalHashes += hashRate
      
      // Simulate finding blocks
      if (Math.random() < 0.0001 * (hashRate / 100)) {
        findBlock()
      }
      
      // Update stats
      powerConsumption = selectedThreads * 25 * (miningIntensity / 100)
      temperature = 45 + (selectedThreads * 3) + (miningIntensity / 10) + (Math.random() * 5)
      efficiency = hashRate / powerConsumption
      
      // Estimate time to next block
      const blockProbability = 0.0001 * (hashRate / 100)
      estimatedTimeToBlock = Math.floor(1 / blockProbability)
    }, 1000)
    
    // Update mining history
    statsInterval = setInterval(() => {
      if (!isMining) return
      
      miningHistory = [...miningHistory.slice(-29), {
        timestamp: Date.now(),
        hashRate: hashRate,
        power: powerConsumption
      }]
    }, 5000)
  }
  
  function stopMining() {
    isMining = false
    hashRate = 0
    
    if (miningInterval) {
      clearInterval(miningInterval)
      miningInterval = null
    }
    
    if (statsInterval) {
      clearInterval(statsInterval)
      statsInterval = null
    }

    // stop uptime ticker
    if (uptimeInterval) {
      clearInterval(uptimeInterval as unknown as number)
      uptimeInterval = null
    }
  }
  
  function findBlock() {
    blocksFound++
    const reward = 5 + Math.random() * 2
    totalRewards += reward
    lastBlockTime = Date.now()
    
    recentBlocks = [{
      id: `block-${Date.now()}`,
      hash: `0x${Math.random().toString(16).substring(2, 10)}...${Math.random().toString(16).substring(2, 6)}`,
      reward: reward,
      timestamp: new Date(),
      difficulty: currentDifficulty,
      nonce: Math.floor(Math.random() * 1000000)
    }, ...recentBlocks.slice(0, 4)]
  }
  
  function formatUptime(now: number = Date.now()) {
    const uptime = now - sessionStartTime
    const hours = Math.floor(uptime / 3600000)
    const minutes = Math.floor((uptime % 3600000) / 60000)
    const seconds = Math.floor((uptime % 60000) / 1000)
    return `${hours}h ${minutes}m ${seconds}s`
  }
  
  function formatHashRate(rate: number): string {
    if (rate < 1000) return `${rate.toFixed(1)} H/s`
    if (rate < 1000000) return `${(rate / 1000).toFixed(2)} KH/s`
    if (rate < 1000000000) return `${(rate / 1000000).toFixed(2)} MH/s`
    return `${(rate / 1000000000).toFixed(2)} GH/s`
  }
  
  function formatNumber(num: number): string {
    if (num < 1000) return num.toString()
    if (num < 1000000) return `${(num / 1000).toFixed(1)}K`
    if (num < 1000000000) return `${(num / 1000000).toFixed(1)}M`
    return `${(num / 1000000000).toFixed(1)}B`
  }
  
  // Mock pool options
  const pools = [
    { value: 'solo', label: 'Solo Mining' },
    { value: 'pool1', label: 'ChiralPool #1' },
    { value: 'pool2', label: 'ChiralPool #2' },
    { value: 'pool3', label: 'Community Pool' }
  ]
  
  onDestroy(() => {
    stopMining()
  })
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Mining</h1>
    <p class="text-muted-foreground mt-2">Contribute computing power to secure the network and earn rewards</p>
  </div>
  
  <!-- Mining Status Cards -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Hash Rate</p>
          <p class="text-2xl font-bold">{formatHashRate(hashRate)}</p>
          <p class="text-xs text-muted-foreground mt-1">
            {selectedThreads} threads
          </p>
        </div>
        <Cpu class="h-8 w-8 text-muted-foreground" />
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Total Rewards</p>
          <p class="text-2xl font-bold">{totalRewards.toFixed(2)} CN</p>
          <p class="text-xs text-green-600 flex items-center gap-1 mt-1">
            <TrendingUp class="h-3 w-3" />
            {blocksFound} blocks found
          </p>
        </div>
        <Coins class="h-8 w-8 text-muted-foreground" />
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Power Usage</p>
          <p class="text-2xl font-bold">{powerConsumption.toFixed(0)}W</p>
          <p class="text-xs text-muted-foreground mt-1">
            {efficiency.toFixed(2)} H/W
          </p>
        </div>
        <Zap class="h-8 w-8 text-muted-foreground" />
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Temperature</p>
          <p class="text-2xl font-bold">{temperature.toFixed(1)}°C</p>
          <div class="mt-1">
            <Progress 
              value={temperature} 
              max={100} 
              class="h-1 {temperature > 80 ? 'bg-red-500' : temperature > 60 ? 'bg-yellow-500' : ''}"
            />
          </div>
        </div>
        <div class="text-2xl">🌡️</div>
      </div>
    </Card>
  </div>
  
  <!-- Mining Control -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Mining Control</h2>
      <Badge variant={isMining ? 'default' : 'secondary'}>
        {isMining ? 'Mining Active' : 'Mining Stopped'}
      </Badge>
    </div>
    
    <div class="space-y-4">
      <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div class="relative">
          <Label for="pool-select">Mining Pool</Label>
          <select
            id="pool-select"
            bind:value={miningPool}
            disabled={isMining}
            class="w-full mt-2 px-3 py-2 border rounded-lg bg-background appearance-none"
          >
            {#each pools as pool}
              <option value={pool.value}>{pool.label}</option>
            {/each}
          </select>
          <ChevronsUpDown
            class="pointer-events-none absolute right-2 mt-4 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
          />
        </div>
        
        <div>
          <Label for="thread-count">CPU Threads ({cpuThreads} available)</Label>
          <Input
            id="thread-count"
            type="number"
            bind:value={selectedThreads}
            min="1"
            max={cpuThreads}
            disabled={isMining}
            class="mt-2"
          />
        </div>
        
        <div>
          <Label for="intensity">Mining Intensity (%)</Label>
          <Input
            id="intensity"
            type="number"
            bind:value={miningIntensity}
            min="10"
            max="100"
            step="10"
            disabled={isMining}
            class="mt-2"
          />
        </div>
      </div>
      
      <div class="flex items-center justify-between pt-4">
        <div class="text-sm space-y-1">
          <p class="text-muted-foreground">
            Session: <span class="font-medium">{isMining ? formatUptime() : '0h 0m 0s'}</span>
          </p>
          <p class="text-muted-foreground">
            Total Hashes: <span class="font-medium">{formatNumber(totalHashes)}</span>
          </p>
        </div>
        
        <Button
          size="lg"
          on:click={() => isMining ? stopMining() : startMining()}
          class="min-w-[150px]"
        >
          {#if isMining}
            <Pause class="h-4 w-4 mr-2" />
            Stop Mining
          {:else}
            <Play class="h-4 w-4 mr-2" />
            Start Mining
          {/if}
        </Button>
      </div>
    </div>
  </Card>
  
  <!-- Mining Statistics -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Network Statistics</h2>
      <div class="space-y-3">
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Network Hash Rate</span>
          <Badge variant="outline">2.4 GH/s</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Network Difficulty</span>
          <Badge variant="outline">{currentDifficulty}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Block Height</span>
          <Badge variant="outline">#{245789 + blocksFound}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Block Reward</span>
          <Badge variant="outline">5 CN</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Est. Time to Block</span>
          <Badge variant="outline">
            {estimatedTimeToBlock > 0 ? `~${Math.floor(estimatedTimeToBlock / 60)} min` : 'Calculating...'}
          </Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Active Miners</span>
          <Badge variant="outline">1,247</Badge>
        </div>
      </div>
    </Card>
    
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Pool Information</h2>
      {#if miningPool === 'solo'}
        <div class="space-y-3">
          <p class="text-sm text-muted-foreground">
            You are mining solo. All block rewards will be yours, but finding blocks may take longer.
          </p>
          <div class="pt-2 space-y-2">
            <div class="flex justify-between">
              <span class="text-sm">Your Share</span>
              <span class="text-sm font-medium">100%</span>
            </div>
            <div class="flex justify-between">
              <span class="text-sm">Pool Fee</span>
              <span class="text-sm font-medium">0%</span>
            </div>
            <div class="flex justify-between">
              <span class="text-sm">Min Payout</span>
              <span class="text-sm font-medium">N/A</span>
            </div>
          </div>
        </div>
      {:else}
        <div class="space-y-3">
          <div class="flex justify-between">
            <span class="text-sm">Pool Hash Rate</span>
            <span class="text-sm font-medium">850 MH/s</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Pool Miners</span>
            <span class="text-sm font-medium">342</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Your Share</span>
            <span class="text-sm font-medium">{(hashRate / 850000000 * 100).toFixed(4)}%</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Pool Fee</span>
            <span class="text-sm font-medium">1%</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Min Payout</span>
            <span class="text-sm font-medium">10 CN</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Payment Method</span>
            <span class="text-sm font-medium">PPLNS</span>
          </div>
        </div>
      {/if}
    </Card>
  </div>
  
  <!-- Recent Blocks -->
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">Recent Blocks Found</h2>
    {#if recentBlocks.length === 0}
      <p class="text-sm text-muted-foreground text-center py-8">
        No blocks found yet. Start mining to earn rewards!
      </p>
    {:else}
      <div class="space-y-2">
        {#each recentBlocks as block}
          <div class="flex items-center justify-between p-3 bg-secondary rounded-lg">
            <div class="flex items-center gap-3">
              <Award class="h-4 w-4 text-yellow-500" />
              <div>
                <p class="text-sm font-medium">Block Found!</p>
                <p class="text-xs text-muted-foreground">
                  Hash: {block.hash} • Nonce: {block.nonce}
                </p>
              </div>
            </div>
            <div class="text-right">
              <Badge variant="outline" class="text-green-600">
                +{block.reward.toFixed(2)} CN
              </Badge>
              <p class="text-xs text-muted-foreground mt-1">
                {block.timestamp.toLocaleTimeString()}
              </p>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </Card>
  
  <!-- Hash Rate Chart (simplified) -->
  {#if miningHistory.length > 0}
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Hash Rate History</h2>
      <div class="h-32 flex items-end gap-1">
        {#each miningHistory as point}
          <div 
            class="flex-1 bg-primary/20 hover:bg-primary/30 transition-colors rounded-t"
            style="height: {(point.hashRate / Math.max(...miningHistory.map(h => h.hashRate))) * 100}%"
            title="{formatHashRate(point.hashRate)}"
          ></div>
        {/each}
      </div>
      <p class="text-xs text-muted-foreground text-center mt-2">Last 5 minutes</p>
    </Card>
  {/if}
</div>