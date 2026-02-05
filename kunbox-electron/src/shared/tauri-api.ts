// Tauri API adapter - provides the same interface as Electron's window.api
import { invoke } from '@tauri-apps/api/core';
import { listen, emit } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { AppSettings, Profile, SingBoxOutbound, ProxyState, TrafficStats, LogEntry } from './types';

// Event listener storage for cleanup
const eventListeners = new Map<string, () => void>();

export const api = {
  singbox: {
    start: () => invoke<{ success: boolean; error?: string }>('singbox_start'),
    stop: () => invoke<{ success: boolean; error?: string }>('singbox_stop'),
    restart: () => invoke<{ success: boolean; error?: string }>('singbox_restart'),
    switchNode: (nodeTag: string) => invoke<{ success: boolean; error?: string }>('singbox_switch_node', { nodeTag }),
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
    }
  },

  tray: {
    onToggleConnection: (callback: () => void) => {
      const unlisten = listen('tray:toggle-connection', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onRestartCore: (callback: () => void) => {
      const unlisten = listen('tray:restart-core', () => callback());
      return () => { unlisten.then(fn => fn()); };
    },
    onSetMode: (callback: (mode: 'rule' | 'global' | 'direct') => void) => {
      const unlisten = listen<string>('tray:set-mode', (event) => {
        callback(event.payload as 'rule' | 'global' | 'direct');
      });
      return () => { unlisten.then(fn => fn()); };
    },
    updateStatus: (_connected: boolean) => {
      // Tauri handles tray updates differently - emit event to backend
      emit('tray:status-update', { connected: _connected });
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
    export: (tag: string): Promise<string> => invoke('node_export', { tag })
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
    getInstalledVersions: () => Promise.resolve([]),
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
    clearCache: () => invoke<{ success: boolean }>('kernel_clear_cache'),
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
    list: () => invoke('ruleset_list'),
    save: (ruleSets: any[]) => invoke('ruleset_save', { rulesets: ruleSets }),
    download: (ruleSet: any) => invoke('ruleset_download', { ruleset: ruleSet }),
    isCached: (tag: string) => invoke('ruleset_is_cached', { tag }),
    fetchHub: () => invoke<{ tree: Array<{ type: string; path: string }> }>('ruleset_fetch_hub')
  },

  window: {
    minimize: () => invoke('window_minimize'),
    maximize: () => invoke('window_maximize'),
    close: () => invoke('window_close')
  }
};

// Initialize Tauri API on window
export function initTauriApi() {
  (window as any).api = api;
}

export type API = typeof api;
