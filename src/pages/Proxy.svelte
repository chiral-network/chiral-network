<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { Shield, Globe, Activity, Plus, Power, Trash2 } from 'lucide-svelte'
  import { proxyNodes } from '$lib/stores'
  
  let newNodeAddress = ''
  let proxyEnabled = true
  let isAddressValid = true
  const validAddressRegex = /^[a-zA-Z0-9.-]+:[0-9]{1,5}$/

  function addNode() {
      const validAddressRegex = /^[a-zA-Z0-9.-]+:[0-9]{1,5}$/
      const isDuplicate = $proxyNodes.some(node => node.address === newNodeAddress.trim())
      if (isDuplicate) {
          alert('This proxy address is already added!')
          return
      }

      if (!newNodeAddress || !validAddressRegex.test(newNodeAddress.trim())) {
          alert('Please enter a valid proxy address (e.g., 192.168.1.100:8080)')
          return
      }

      const newNode = {
          id: `node-${Date.now()}`,
          address: newNodeAddress.trim(),
          status: 'connecting' as const,
          bandwidth: 0,
          latency: 999,
          region: 'Unknown'
      }

      proxyNodes.update(nodes => [...nodes, newNode])
      newNodeAddress = ''

      // Simulate connection
      setTimeout(() => {
          proxyNodes.update(nodes => nodes.map(node => {
              if (node.id === newNode.id) {
                  return {
                      ...node,
                      status: 'online',
                      bandwidth: Math.floor(Math.random() * 100),
                      latency: Math.floor(Math.random() * 100)
                  }
              }
              return node
          }))
      }, 2000)
  }


  function removeNode(nodeId: string) {
    proxyNodes.update(nodes => nodes.filter(node => node.id !== nodeId))
  }
  
  function toggleNode(nodeId: string) {
    proxyNodes.update(nodes => nodes.map(node => {
      if (node.id === nodeId) {
        return {
          ...node,
          status: node.status === 'online' ? 'offline' : 'online'
        }
      }
      return node
    }))
  }
  
  $: activeNodes = $proxyNodes.filter(n => n.status === 'online').length
  $: totalBandwidth = $proxyNodes.reduce((sum, n) => sum + (n.status === 'online' ? n.bandwidth : 0), 0)
  $: isAddressValid = validAddressRegex.test(newNodeAddress.trim())
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Proxy Network</h1>
    <p class="text-muted-foreground mt-2">Manage your proxy nodes and network settings</p>
  </div>
  
  <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-primary/10 rounded-lg">
          <Shield class="h-5 w-5 text-primary" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">Proxy Status</p>
          <p class="text-xl font-bold">{proxyEnabled ? 'Active' : 'Inactive'}</p>
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-green-500/10 rounded-lg">
          <Globe class="h-5 w-5 text-green-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">Active Nodes</p>
          <p class="text-xl font-bold">{activeNodes} / {$proxyNodes.length}</p>
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-blue-500/10 rounded-lg">
          <Activity class="h-5 w-5 text-blue-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">Total Bandwidth</p>
          <p class="text-xl font-bold">{totalBandwidth} Mbps</p>
        </div>
      </div>
    </Card>
  </div>
  
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Proxy Settings</h2>
      <Button
        variant={proxyEnabled ? 'default' : 'outline'}
        size="sm"
        on:click={() => proxyEnabled = !proxyEnabled}
      >
        <Power class="h-4 w-4 mr-2" />
        {proxyEnabled ? 'Disable' : 'Enable'} Proxy
      </Button>
    </div>
    
    <div class="space-y-4">
        <div>
            <Label for="new-node">Add Proxy Node</Label>
            <div class="flex gap-2 mt-2">
                <Input
                    id="new-node"
                    bind:value={newNodeAddress}
                    placeholder="Enter node address (e.g., 192.168.1.100:8080)"
                    class="flex-1 {isAddressValid || newNodeAddress === '' ? '' : 'border border-red-500 focus:ring-red-500'}"
                />
                <Button on:click={addNode} disabled={!isAddressValid || !newNodeAddress}>
                    <Plus class="h-4 w-4 mr-2" />
                    Add Node
                </Button>
            </div>
            {#if !isAddressValid && newNodeAddress !== ''}
                <p class="text-sm text-red-500 mt-1">Please enter a valid proxy address (e.g., 192.168.1.100:8080)</p>
            {/if}
        </div>

    </div>
  </Card>
  
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">Proxy Nodes</h2>
    <div class="space-y-3">
      {#each $proxyNodes as node}
        <div class="p-4 bg-secondary rounded-lg">
          <div class="flex items-center justify-between mb-3">
            <div class="flex items-center gap-3">
              <div class="w-2 h-2 rounded-full {
                node.status === 'online' ? 'bg-green-500' : 
                node.status === 'offline' ? 'bg-red-500' : 
                node.status === 'connecting' ? 'bg-yellow-500' :
                'bg-gray-500'
              }"></div>
              <div>
                <p class="font-medium">{node.address}</p>
                <p class="text-xs text-muted-foreground">{node.region}</p>
              </div>
            </div>
              <Badge variant={node.status === 'online' ? 'default' :
                   node.status === 'offline' ? 'secondary' :
                   node.status === 'connecting' ? 'outline' : 'outline'}
                      class={
                        node.status === 'online' ? 'bg-green-500 text-white' :
                        node.status === 'offline' ? 'bg-red-500 text-white' :
                        node.status === 'connecting' ? 'bg-yellow-500 text-white' :
                        'bg-gray-500 text-white'
                      }
                      style="pointer-events: none;"
              >
              {node.status}
            </Badge>
          </div>
          
          <div class="grid grid-cols-2 gap-4 mb-3">
            <div>
              <p class="text-xs text-muted-foreground">Bandwidth</p>
              <p class="text-sm font-medium">{node.bandwidth} Mbps</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">Latency</p>
              <p class="text-sm font-medium">{node.latency} ms</p>
            </div>
          </div>
          
          <div class="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              on:click={() => toggleNode(node.id)}
            >
              <Power class="h-3 w-3 mr-1" />
              {node.status === 'online' ? 'Disconnect' : 'Connect'}
            </Button>
            <Button
              size="sm"
              variant="destructive"
              on:click={() => removeNode(node.id)}
            >
              <Trash2 class="h-3 w-3 mr-1" />
              Remove
            </Button>
          </div>
        </div>
      {/each}
    </div>
  </Card>
</div>