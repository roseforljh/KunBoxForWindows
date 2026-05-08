// Tauri API adapter - provides the same interface as Electron's window.api
import { invoke } from '@tauri-apps/api/core';
import { listen, emit } from '@tauri-apps/api/event';
import type { AppSettings, Profile, SingBoxOutbound, ProxyState, TrafficStats, LogEntry, DomainRule, CustomRules, NodeWithProfile, NodeLatencyResult } from './types';

function bindUnlisten(unlistenPromise: Promise<() => void>) {
  let cancelled = false;
  let unlistenFn: (() => void) | null = null;

  void unlistenPromise
    .then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlistenFn = fn;
      }
    })
    .catch((err) => {
      console.error('Failed to bind listener:', err);
    });

  return () => {
    cancelled = true;
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
  };
}

export const api = {
  singbox: {
    start: () => invoke<{ success: boolean; error?: string; warning?: string }>('singbox_start'),
    stop: () => invoke<{ success: boolean; error?: string; warning?: string }>('singbox_stop'),
    restart: () => invoke<{ success: boolean; error?: string; warning?: string }>('singbox_restart'),
    switchNode: (nodeTag: string) => invoke<{ success: boolean; error?: string }>('singbox_switch_node', { nodeTag }),
    enableSystemProxy: (port?: number) => invoke<{ success: boolean; error?: string }>('singbox_enable_system_proxy', { port }),
    disableSystemProxy: () => invoke<{ success: boolean; error?: string }>('singbox_disable_system_proxy'),
    onStateChange: (callback: (state: ProxyState) => void) => {
      const unlisten = listen<string>('singbox:state', (event) => {
        callback(event.payload as ProxyState);
      });
      return bindUnlisten(unlisten);
    },
    onTraffic: (callback: (stats: TrafficStats) => void) => {
      const unlisten = listen<TrafficStats>('singbox:traffic', (event) => {
        callback(event.payload);
      });
      return bindUnlisten(unlisten);
    },
    onLog: (callback: (entry: LogEntry) => void) => {
      const unlisten = listen<LogEntry>('singbox:log', (event) => {
        callback(event.payload);
      });
      return bindUnlisten(unlisten);
    },
    testSelectorLatency: (selectorTag: string, testUrl?: string): Promise<{
      success: boolean;
      selector: string;
      total: number;
      tested: number;
      timeout: number;
      bestNode?: string;
      bestDelay?: number;
    }> => invoke('singbox_test_selector_latency', { selectorTag, testUrl }),
    onSelectorSwitch: (callback: (data: { selector: string; node: string; delay: number; stage: 'first' | 'final' }) => void) => {
      const unlisten = listen<{ selector: string; node: string; delay: number; stage: 'first' | 'final' }>('singbox:selector-switch', (event) => {
        callback(event.payload);
      });
      return bindUnlisten(unlisten);
    }
  },

  tray: {
    updateStatus: (_connected: boolean) => {
      // Tauri handles tray updates differently - emit event to backend
      emit('tray:status-update', { connected: _connected });
    },
    onVpnStart: (callback: () => void) => {
      const unlisten = listen('tray-vpn-start', () => callback());
      return bindUnlisten(unlisten);
    },
    onVpnStop: (callback: () => void) => {
      const unlisten = listen('tray-vpn-stop', () => callback());
      return bindUnlisten(unlisten);
    },
    onVpnRestart: (callback: () => void) => {
      const unlisten = listen('tray-vpn-restart', () => callback());
      return bindUnlisten(unlisten);
    },
    onProxyEnable: (callback: () => void) => {
      const unlisten = listen('tray-proxy-enable', () => callback());
      return bindUnlisten(unlisten);
    },
    onProxyDisable: (callback: () => void) => {
      const unlisten = listen('tray-proxy-disable', () => callback());
      return bindUnlisten(unlisten);
    },
    onTunEnable: (callback: () => void) => {
      const unlisten = listen('tray-tun-enable', () => callback());
      return bindUnlisten(unlisten);
    },
    onTunDisable: (callback: () => void) => {
      const unlisten = listen('tray-tun-disable', () => callback());
      return bindUnlisten(unlisten);
    },
    onQuit: (callback: () => void) => {
      const unlisten = listen('tray-quit', () => callback());
      return bindUnlisten(unlisten);
    }
  },

  profile: {
    list: (): Promise<Profile[]> => invoke('profile_list'),
    add: (url: string, name?: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }): Promise<Profile> => 
      invoke('profile_add', { 
        url, 
        name, 
        autoUpdateInterval: settings?.autoUpdateInterval,
        dnsPreResolve: settings?.dnsPreResolve,
        dnsServer: settings?.dnsServer
      }),
    importContent: (name: string, content: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }): Promise<Profile> =>
      invoke('profile_import_content', {
        name,
        content,
        autoUpdateInterval: settings?.autoUpdateInterval,
        dnsPreResolve: settings?.dnsPreResolve,
        dnsServer: settings?.dnsServer
      }),
    update: (id: string): Promise<Profile> => invoke('profile_update', { id }),
    delete: (id: string): Promise<void> => invoke('profile_delete', { id }),
    getActive: (): Promise<string | null> => invoke('profile_get_active'),
    setActive: (id: string): Promise<void> => invoke('profile_set_active', { id }),
    refresh: (id: string): Promise<Profile> => invoke('profile_update', { id }),
    edit: (id: string, data: { name: string; url: string; autoUpdateInterval?: number; dnsPreResolve?: boolean; dnsServer?: string | null }): Promise<Profile> => 
      invoke('profile_edit', { 
        id, 
        name: data.name, 
        url: data.url,
        autoUpdateInterval: data.autoUpdateInterval,
        dnsPreResolve: data.dnsPreResolve,
        dnsServer: data.dnsServer
      }),
    setEnabled: (id: string, enabled: boolean): Promise<void> => invoke('profile_set_enabled', { id, enabled })
  },

  node: {
    list: (): Promise<SingBoxOutbound[]> => invoke('node_list'),
    setActive: (tag: string): Promise<void> => invoke('node_set_active', { tag }),
    add: (link: string, target?: { type: 'existing'; profileId: string } | { type: 'new'; profileName: string }): Promise<SingBoxOutbound> => {
      if (target?.type === 'new') {
        return invoke('node_add', { link, profileName: target.profileName });
      }
      const profileId = target?.type === 'existing' ? target.profileId : undefined;
      return invoke('node_add', { link, profileId });
    },
    beginLatencyTests: (runId: number): Promise<void> => invoke('node_begin_latency_tests', { runId }),
    testLatency: (tag: string, runId?: number): Promise<NodeLatencyResult> => invoke<NodeLatencyResult>('node_test_latency', { tag, runId }),
    testAll: (): Promise<Record<string, number>> => invoke('node_test_all'),
    cancelLatencyTests: (runId?: number): Promise<void> => invoke('node_cancel_latency_tests', { runId }),
    delete: (tag: string): Promise<void> => invoke('node_delete', { tag }),
    export: (tag: string): Promise<string> => invoke('node_export', { tag }),
    listAll: (): Promise<NodeWithProfile[]> => invoke('node_list_all')
  },

  settings: {
    get: (): Promise<AppSettings> => invoke('get_settings'),
    set: (settings: Partial<AppSettings>): Promise<void> => invoke('set_settings', { settings })
  },

  kernel: {
    getLocalVersion: async (_isAlpha?: boolean) => {
      // Tauri returns KernelVersion | null directly with camelCase
      const result = await invoke<{ version: string; versionDetail: string; isAlpha: boolean } | null>('kernel_get_local_version');
      return result;
    },
    getInstalledVersions: () => invoke<Array<{
      version: string;
      versionDetail: string;
      isBackup: boolean;
      path: string;
    }>>('kernel_get_installed_versions'),
    getCapabilities: () => invoke<{
      version: string;
      supportsNaive: boolean;
      supportsIcmpProxy: boolean;
      supportsBypassAction: boolean;
    }>('kernel_get_capabilities'),
    getRemoteReleases: async (includePrerelease?: boolean) => {
      // Tauri returns RemoteRelease[] directly with camelCase
      const releases = await invoke<Array<{
        version: string;
        tagName: string;
        publishedAt: string;
        isPrerelease: boolean;
        downloadUrl: string;
        assetName: string;
      }>>('kernel_get_remote_releases', { includePrerelease });
      return releases;
    },
    download: (tagName: string) => invoke<{ success: boolean }>('kernel_download', { tagName }),
    rollback: (_isAlpha?: boolean) => invoke<{ success: boolean }>('kernel_rollback'),
    canRollback: (_isAlpha?: boolean) => invoke<boolean>('kernel_can_rollback'),
    clearCache: () => invoke<{ success: boolean; freedBytes: number }>('kernel_clear_cache'),
    openReleasesPage: () => invoke('kernel_open_releases_page'),
    openDirectory: () => invoke('kernel_open_directory'),
    onDownloadProgress: (callback: (progress: { downloaded: number; total: number; percent: number }) => void) => {
      const unlisten = listen<{ downloaded: number; total: number; percent: number }>('kernel:download-progress', (event) => {
        callback(event.payload);
      });
      return bindUnlisten(unlisten);
    },
    onDownloadComplete: (callback: () => void) => {
      const unlisten = listen('kernel:download-complete', () => callback());
      return bindUnlisten(unlisten);
    },
    onDownloadError: (callback: (error: string) => void) => {
      const unlisten = listen<string>('kernel:download-error', (event) => {
        callback(event.payload);
      });
      return bindUnlisten(unlisten);
    }
  },

  ruleset: {
    list: () => invoke<any[]>('ruleset_list'),
    save: (ruleSets: any[]) => invoke('ruleset_save', { rulesets: ruleSets }),
    download: (ruleSet: any) => invoke<{ success: boolean; cached?: boolean; error?: string }>('ruleset_download', { ruleset: ruleSet }),
    isCached: (tag: string) => invoke<boolean>('ruleset_is_cached', { tag }),
    fetchHub: () => invoke<{ tree: Array<{ type: string; path: string }> }>('ruleset_fetch_hub')
  },

  customRules: {
    get: (): Promise<CustomRules> => invoke('custom_rules_get'),
    save: (rules: CustomRules): Promise<void> => invoke('custom_rules_save', { rules }),
    getDomainRules: (): Promise<DomainRule[]> => invoke('domain_rules_get'),
    saveDomainRules: (rules: DomainRule[]): Promise<void> => invoke('domain_rules_save', { rules })
  },

  updater: {
    getCurrentVersion: (): Promise<string> => invoke('updater_get_current_version'),
    check: (): Promise<{ currentVersion: string; hasUpdate: boolean; version?: string; date?: string; body?: string }> => invoke('updater_check'),
    downloadAndInstall: (): Promise<{ currentVersion: string; hasUpdate: boolean; version?: string; date?: string; body?: string }> => invoke('updater_download_and_install'),
    onDownloadProgress: (callback: (progress: { downloaded: number; contentLength: number }) => void) => {
      const unlisten = listen<{ downloaded: number; contentLength: number }>('updater:download-progress', (event) => {
        callback(event.payload);
      });
      return bindUnlisten(unlisten);
    },
    onDownloadFinished: (callback: () => void) => {
      const unlisten = listen('updater:download-finished', () => callback());
      return bindUnlisten(unlisten);
    }
  },

    window: {
    minimize: () => invoke('window_minimize'),
    maximize: () => invoke('window_maximize'),
    close: () => invoke('window_close'),
    show: () => invoke('window_show'),
    restartAsAdmin: () => invoke('restart_as_admin'),
    isAdmin: (): Promise<boolean> => invoke('is_admin')
  }
};

// Initialize Tauri API on window
export function initTauriApi() {
  (window as any).api = api;
}

export type API = typeof api;
