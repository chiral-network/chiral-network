// Service for managing multiple proxies and distributing network load

export class ProxyLoadBalancer {
  private proxies: string[] = [];
  private currentIndex = 0;

  setProxies(proxyList: string[]) {
    this.proxies = proxyList;
    this.currentIndex = 0;
  }

  getNextProxy(): string | null {
    if (this.proxies.length === 0) return null;
    const proxy = this.proxies[this.currentIndex];
    this.currentIndex = (this.currentIndex + 1) % this.proxies.length;
    return proxy;
  }

  getAllProxies(): string[] {
    return this.proxies;
  }

  // Add proxies with weights for smarter load balancing
  setWeightedProxies(proxyList: {address: string, weight: number}[]) {
    // Sort proxies by weight descending
    this.proxies = proxyList
      .sort((a, b) => b.weight - a.weight)
      .map(p => p.address);
    this.currentIndex = 0;
  }

  // Get the proxy with the highest weight (for prioritized routing)
  getHighestWeightProxy(): string | null {
    if (this.proxies.length === 0) return null;
    return this.proxies[0];
  }
}

export const proxyLoadBalancer = new ProxyLoadBalancer();
