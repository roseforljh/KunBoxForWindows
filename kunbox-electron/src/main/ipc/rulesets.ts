import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs'
import log from 'electron-log'
import https from 'https'
import { IPC_CHANNELS } from '../../shared/constants'
import { downloadRuleSet, isRuleSetLocal, type RuleSetItem } from '../utils/ruleSetManager'

import { getDataDir } from '../utils/paths'

const DATA_DIR = getDataDir()
const RULESETS_FILE = join(DATA_DIR, 'rulesets.json')

const defaultRuleSets: RuleSetItem[] = [
  {
    id: '1',
    tag: 'geosite-cn',
    name: '中国网站',
    url: 'https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs',
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
    url: 'https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs',
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
    url: 'https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-private.srs',
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
    url: 'https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs',
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

  ipcMain.handle(IPC_CHANNELS.RULESET_DOWNLOAD, async (_, ruleSet: RuleSetItem, force = false) => {
    if (ruleSet.type !== 'remote') {
      return { success: true, cached: true }
    }

    if (!force && isRuleSetLocal(ruleSet.tag)) {
      return { success: true, cached: true }
    }

    log.info(`[RuleSet] Downloading ${ruleSet.tag}...`)
    const success = await downloadRuleSet(ruleSet)
    if (success) {
      return { success: true, cached: false }
    } else {
      return { success: false, error: `Failed to download ${ruleSet.tag}` }
    }
  })

  ipcMain.handle(IPC_CHANNELS.RULESET_IS_CACHED, (_, tag: string) => {
    return isRuleSetLocal(tag)
  })

  ipcMain.handle('ruleset:fetch-hub', async () => {
    return new Promise((resolve) => {
      https.get('https://api.github.com/repos/MetaCubeX/meta-rules-dat/git/trees/meta?recursive=1', {
        headers: { 'User-Agent': 'KunBox/0.0.24' }
      }, (res) => {
        let body = ''
        res.on('data', chunk => body += chunk)
        res.on('end', () => {
          try {
            resolve(JSON.parse(body))
          } catch {
            resolve({ tree: [] })
          }
        })
      }).on('error', () => resolve({ tree: [] }))
    })
  })
}
