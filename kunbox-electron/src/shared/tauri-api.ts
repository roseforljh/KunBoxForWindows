// Tauri API adapter - provides the same interface as Electron's window.api
import { invoke } from '@tauri-apps/api/core';
import { listen, emit } from '@tauri-apps/api/event';
import type { AppSettings, Profile, SingBoxOutbound, ProxyState, TrafficStats, LogEntry, DomainRule, CustomRules, NodeWithProfile } from './types';

export const api = {
  singbox: {
    start: () => invoke<{ success: boolean; error?: string }>('singbox_start'),
    stop: () => invoke<{ success: boolean; error?: string }>('singbox_stop'),
    restart: () => invoke<{ success: boolean; error?: string }>('singbox_restart'),
    switchNode: (nodeTag: string) => invoke<{ success: boolean; error?: string }>('singbox_switch_node', { nodeTag }),
    enableSystemProxy: (port?: number) => invoke<{ success: boolean; error?: string }>('singbox_enable_system_proxy', { port }),
    disableSystemProxy: () => invoke<{ success: boolean; error?: string }>('singbox_disable_system_proxy'),
    onStateChange: (callback: (state: ProxyState) => void) => {
      const unlisten = listen<string>('singbox:state', (event) => {
        callback(event.payload as ProxyState);
      });
      return () => { unlisten.then(fn => fn()); };
    },
    onTraffic: (callback: (stats: TrafficStats) => void) => {
      const unlisten = listen<TrafficStats>('singbox:traffic', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then(fn => fn()); };
    },
    onLog: (callback: (entry: LogEntry) => void) => {
      const unlisten = listen<LogEntry>('singbox:log', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then(fn => fn()); };
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
      return () => { unlisten.then(fn => fn()); };
    }
  },

  tray: {
    updateStatus: (_connected: boolean) => {
      // Tauri handles tray updates differently - emit event to backend
      emit('tray:status-update', { connected: _connected });
    },
    onVpnStart: (callback: () => void) => {
      const unlisten = listen('tray-vpn-start', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onVpnStop: (callback: () => void) => {
      const unlisten = listen('tray-vpn-stop', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onVpnRestart: (callback: () => void) => {
      const unlisten = listen('tray-vpn-restart', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onProxyEnable: (callback: () => void) => {
      const unlisten = listen('tray-proxy-enable', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onProxyDisable: (callback: () => void) => {
      const unlisten = listen('tray-proxy-disable', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onTunEnable: (callback: () => void) => {
      const unlisten = listen('tray-tun-enable', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onTunDisable: (callback: () => void) => {
      const unlisten = listen('tray-tun-disable', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onQuit: (callback: () => void) => {
      const unlisten = listen('tray-quit', () => callback());
      return () => { unlisten.then(fn => fn()); };
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
      const profileId = target?.type === 'existing' ? target.profileId : undefined;
      return invoke('node_add', { link, profileId });
    },
    testLatency: (tag: string): Promise<number> => invoke<number>('node_test_latency', { tag }),
    testAll: (): Promise<Record<string, number>> => invoke('node_test_all'),
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
    download: (release: any, _isAlpha?: boolean) => invoke<{ success: boolean }>('kernel_download', { release }),
    rollback: (_isAlpha?: boolean) => invoke<{ success: boolean }>('kernel_rollback'),
    canRollback: (_isAlpha?: boolean) => invoke<boolean>('kernel_can_rollback'),
    clearCache: () => invoke<{ success: boolean; freedBytes: number }>('kernel_clear_cache'),
    openReleasesPage: () => invoke('kernel_open_releases_page'),
    openDirectory: () => invoke('kernel_open_directory'),
    onDownloadProgress: (callback: (progress: { downloaded: number; total: number; percent: number }) => void) => {
      const unlisten = listen<{ downloaded: number; total: number; percent: number }>('kernel:download-progress', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then(fn => fn()); };
    },
    onDownloadComplete: (callback: () => void) => {
      const unlisten = listen('kernel:download-complete', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onDownloadError: (callback: (error: string) => void) => {
      const unlisten = listen<string>('kernel:download-error', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then(fn => fn()); };
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
    check: (): Promise<{ currentVersion: string; hasUpdate: boolean; version?: string; date?: string; body?: string }> => invoke('updater_check'),
    downloadAndInstall: (): Promise<{ currentVersion: string; hasUpdate: boolean; version?: string; date?: string; body?: string }> => invoke('updater_download_and_install'),
    onDownloadProgress: (callback: (progress: { chunkLength: number; contentLength: number }) => void) => {
      const unlisten = listen<{ chunkLength: number; contentLength: number }>('updater:download-progress', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then(fn => fn()); };
    },
    onDownloadFinished: (callback: () => void) => {
      const unlisten = listen('updater:download-finished', () => callback());
      return () => { unlisten.then(fn => fn()); };
    }
  },

  window: {
    minimize: () => invoke('window_minimize'),
    maximize: () => invoke('window_maximize'),
    close: () => invoke('window_close'),
    listRunningProcesses: (): Promise<string[]> => invoke('list_running_processes'),
    restartAsAdmin: () => invoke('restart_as_admin'),
    isAdmin: (): Promise<boolean> => invoke('is_admin'),
    quit: () => invoke('quit_app')
  }
};

// Initialize Tauri API on window
export function initTauriApi() {
  (window as any).api = api;
}

export type API = typeof api;
