import { invoke } from '@tauri-apps/api/core';

export interface ProxyLatencyInfo {
  proxyId: string;
  address: string;
  latencyMs?: number;
  lastUpdated: number;
  status: 'Online' | 'Offline' | 'Connecting' | 'Error';
}

export class ProxyLatencyOptimizationService {
  /**
   * Check if Tauri is available by attempting to call invoke
   */
  static async isTauriAvailable(): Promise<boolean> {
    try {
      // Try a simple Tauri command to test availability
      await invoke('get_proxy_optimization_status');
      return true;
    } catch (error) {
      console.warn('Tauri API not available:', error);
      return false;
    }
  }

  /**
   * Update latency information for a proxy
   */
  static async updateProxyLatency(proxyId: string, latencyMs?: number): Promise<void> {
    try {
      return await invoke('update_proxy_latency', { proxyId, latencyMs });
    } catch (error) {
      throw new Error(`Failed to update proxy latency: ${error}`);
    }
  }

  /**
   * Get current proxy optimization status
   */
  static async getOptimizationStatus(): Promise<boolean> {
    try {
      return await invoke('get_proxy_optimization_status');
    } catch (error) {
      throw new Error(`Failed to get optimization status: ${error}`);
    }
  }

  /**
   * Monitor proxy latencies and automatically update the optimization service
   */
  static async startLatencyMonitoring(proxyNodes: any[]): Promise<void> {
    try {
      const isAvailable = await this.isTauriAvailable();
      if (!isAvailable) {
        console.warn('Tauri API not available, skipping latency monitoring');
        return;
      }
      
      for (const proxy of proxyNodes) {
        try {
          if (proxy.status === 'online' && proxy.latency) {
            await this.updateProxyLatency(proxy.id, proxy.latency);
          } else {
            await this.updateProxyLatency(proxy.id, undefined);
          }
        } catch (error) {
          console.warn(`Failed to update latency for proxy ${proxy.id}:`, error);
        }
      }
    } catch (error) {
      console.warn('Failed to start latency monitoring:', error);
    }
  }

  /**
   * Get optimization status message for UI display
   */
  static async getOptimizationStatusMessage(): Promise<string> {
    try {
      const isAvailable = await this.isTauriAvailable();
      if (!isAvailable) {
        return "⚠️ Running in browser mode - Tauri API unavailable";
      }
      
      const isOptimized = await this.getOptimizationStatus();
      return isOptimized 
        ? "✅ Proxy latency optimization enabled"
        : "⚠️ No optimal proxies available";
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      return `❌ Error: ${errorMessage}`;
    }
  }

  /**
   * Log proxy performance for debugging
   */
  static logProxyPerformance(proxyId: string, latencyMs?: number): void {
    if (latencyMs !== undefined) {
      console.log(`🚀 Proxy ${proxyId} latency: ${latencyMs}ms`);
    } else {
      console.log(`❌ Proxy ${proxyId} offline or unavailable`);
    }
  }
}