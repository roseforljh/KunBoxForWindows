export const APP_NAME = 'KunBox'
export const APP_VERSION = '1.0.0'

export const IPC_CHANNELS = {
  SINGBOX_START: 'singbox:start',
  SINGBOX_STOP: 'singbox:stop',
  SINGBOX_RESTART: 'singbox:restart',
  SINGBOX_STATE: 'singbox:state',
  SINGBOX_TRAFFIC: 'singbox:traffic',
  SINGBOX_LOG: 'singbox:log',
  SINGBOX_SWITCH_NODE: 'singbox:switch-node',

  PROFILE_LIST: 'profile:list',
  PROFILE_ADD: 'profile:add',
  PROFILE_IMPORT_CONTENT: 'profile:import-content',
  PROFILE_UPDATE: 'profile:update',
  PROFILE_DELETE: 'profile:delete',
  PROFILE_SET_ACTIVE: 'profile:set-active',
  PROFILE_REFRESH: 'profile:refresh',
  PROFILE_EDIT: 'profile:edit',
  PROFILE_SET_ENABLED: 'profile:set-enabled',

  NODE_LIST: 'node:list',
  NODE_SET_ACTIVE: 'node:set-active',
  NODE_ADD: 'node:add',
  NODE_TEST_LATENCY: 'node:test-latency',
  NODE_TEST_ALL: 'node:test-all',
  NODE_DELETE: 'node:delete',
  NODE_EXPORT: 'node:export',

  SETTINGS_GET: 'settings:get',
  SETTINGS_SET: 'settings:set',

  RULESET_LIST: 'ruleset:list',
  RULESET_SAVE: 'ruleset:save',
  RULESET_DOWNLOAD: 'ruleset:download',
  RULESET_IS_CACHED: 'ruleset:is-cached',

  WINDOW_MINIMIZE: 'window:minimize',
  WINDOW_MAXIMIZE: 'window:maximize',
  WINDOW_CLOSE: 'window:close'
} as const

export const CLASH_API_URL = 'http://127.0.0.1:9090'

export const SINGBOX_DOWNLOAD_URL = 'https://github.com/SagerNet/sing-box/releases/latest/download/sing-box-windows-amd64.zip'
