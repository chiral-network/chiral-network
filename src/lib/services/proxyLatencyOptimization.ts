import { invoke } from '@tauri-apps/api/core';

export interface ProxyLatencyInfo {
  proxyId: string;
  latencyMs?: number;
  lastUpdated: number;
  status: 'Online' | 'Offline' | 'Connecting' | 'Error';
}

export interface ProxyOptimizationSummary {
  totalProxies: number;
  onlineProxies: number;
  testedProxies: number;
  bestProxyId?: string;
  bestLatencyMs?: number;
  averageLatencyMs?: number;
  shouldUseProxyRouting: boolean;
}

export interface ProxyOptimizationDetail {
  summary: ProxyOptimizationSummary;
  topProxies: ProxyLatencyInfo[];
}

export interface ProxySelfTestResult {
  id: string;
  address: string;
  ok: boolean;
  latencyMs?: number;
  error?: string;
  testedAt: number;
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
      const raw = await invoke<any>('get_proxy_optimization_status');
      if (typeof raw === 'boolean') {
        return raw;
      }
      if (raw && typeof raw === 'object') {
        if (typeof raw.shouldUseProxyRouting === 'boolean') {
          return raw.shouldUseProxyRouting;
        }
        if (raw.summary && typeof raw.summary.shouldUseProxyRouting === 'boolean') {
          return raw.summary.shouldUseProxyRouting;
        }
      }
      return false;
    } catch (error) {
      throw new Error(`Failed to get optimization status: ${error}`);
    }
  }

  static async getOptimizationDetail(): Promise<ProxyOptimizationDetail | null> {
    try {
      const raw = await invoke<any>('get_proxy_optimization_status');
      if (!raw || typeof raw !== 'object' || !raw.summary) {
        return null;
      }
      return raw as ProxyOptimizationDetail;
    } catch (error) {
      throw new Error(`Failed to get optimization detail: ${error}`);
    }
  }

  static async getLatencySnapshot(limit = 50): Promise<ProxyLatencyInfo[]> {
    try {
      return await invoke<ProxyLatencyInfo[]>('get_proxy_latency_snapshot', { limit });
    } catch (error) {
      throw new Error(`Failed to get proxy latency snapshot: ${error}`);
    }
  }

  static async selfTestProxy(target: string, timeoutMs = 1500): Promise<ProxySelfTestResult> {
    try {
      return await invoke<ProxySelfTestResult>('proxy_self_test', { target, timeoutMs });
    } catch (error) {
      throw new Error(`Failed to self-test proxy: ${error}`);
    }
  }

  static async selfTestAll(timeoutMs = 1500): Promise<ProxySelfTestResult[]> {
    try {
      return await invoke<ProxySelfTestResult[]>('proxy_self_test_all', { timeoutMs });
    } catch (error) {
      throw new Error(`Failed to self-test all proxies: ${error}`);
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
