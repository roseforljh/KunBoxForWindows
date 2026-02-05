import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs'
import log from 'electron-log'
import { IPC_CHANNELS } from '../../shared/constants'
import { downloadRuleSet, isRuleSetLocal } from '../utils/ruleSetManager'

const DATA_DIR = join(process.env.APPDATA || '', 'KunBox')
const RULESETS_FILE = join(DATA_DIR, 'rulesets.json')

export interface RuleSetItem {
  id: string
  tag: string
  name: string
  url: string
  type: 'remote' | 'local'
  format: 'binary' | 'source'
  outboundMode: 'direct' | 'proxy' | 'block' | 'node' | 'profile'
  outboundValue?: string
  enabled: boolean
  isBuiltIn: boolean
}

const defaultRuleSets: RuleSetItem[] = [
  {
    id: '1',
    tag: 'geosite-cn',
    name: '中国网站',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'direct',
    enabled: false,
    isBuiltIn: true
  },
  {
    id: '2',
    tag: 'geoip-cn',
    name: '中国 IP',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'direct',
    enabled: false,
    isBuiltIn: true
  },
  {
    id: '3',
    tag: 'geosite-private',
    name: '私有地址',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-private.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'direct',
    enabled: false,
    isBuiltIn: true
  },
  {
    id: '4',
    tag: 'geosite-category-ads-all',
    name: '广告拦截',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'block',
    enabled: false,
    isBuiltIn: true
  }
]

export function loadRuleSets(): RuleSetItem[] {
  try {
    mkdirSync(DATA_DIR, { recursive: true })
    if (existsSync(RULESETS_FILE)) {
      const data = readFileSync(RULESETS_FILE, 'utf-8')
      return JSON.parse(data)
    }
  } catch {
    // ignore
  }
  return defaultRuleSets
}

export function saveRuleSets(ruleSets: RuleSetItem[]): void {
  mkdirSync(DATA_DIR, { recursive: true })
  writeFileSync(RULESETS_FILE, JSON.stringify(ruleSets, null, 2))
}

export function initRuleSetsHandlers(): void {
  ipcMain.handle(IPC_CHANNELS.RULESET_LIST, () => {
    return loadRuleSets()
  })

  ipcMain.handle(IPC_CHANNELS.RULESET_SAVE, (_, ruleSets: RuleSetItem[]) => {
    saveRuleSets(ruleSets)
    return { success: true }
  })

  // Download a single ruleset
  ipcMain.handle(IPC_CHANNELS.RULESET_DOWNLOAD, async (_, ruleSet: RuleSetItem) => {
    if (ruleSet.type !== 'remote') {
      return { success: true, cached: true }
    }

    // Check if already cached
    if (isRuleSetLocal(ruleSet.tag)) {
      log.info(`[RuleSet] ${ruleSet.tag} already cached`)
      return { success: true, cached: true }
    }

    // Download
    log.info(`[RuleSet] Downloading ${ruleSet.tag}...`)
    const success = await downloadRuleSet(ruleSet)
    
    if (success) {
      log.info(`[RuleSet] ${ruleSet.tag} downloaded successfully`)
      return { success: true, cached: false }
    } else {
      log.error(`[RuleSet] Failed to download ${ruleSet.tag}`)
      return { success: false, error: `Failed to download ${ruleSet.tag}` }
    }
  })

  // Check if ruleset is cached locally
  ipcMain.handle(IPC_CHANNELS.RULESET_IS_CACHED, (_, tag: string) => {
    return isRuleSetLocal(tag)
  })
}
