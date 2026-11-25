import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs'
import type { CustomRules, DomainRule } from '../../shared/types'
import { domainRulesNeedRestart, serializeDynamicDomainRules, syncCustomDomainRuleSets } from '../utils/ruleSetManager'
import { getServiceInstance } from './singbox'

import { getDataDir } from '../utils/paths'

const DATA_DIR = getDataDir()
const CUSTOM_RULES_FILE = join(DATA_DIR, 'custom_rules.json')

function loadCustomRules(): CustomRules {
  try {
    mkdirSync(DATA_DIR, { recursive: true })
    if (existsSync(CUSTOM_RULES_FILE)) {
      const data = readFileSync(CUSTOM_RULES_FILE, 'utf-8')
      const parsed = JSON.parse(data)
      if (Array.isArray(parsed.domainRules)) {
        return parsed
      }
    }
  } catch {
    // ignore
  }
  return { domainRules: [] }
}

function saveCustomRules(rules: CustomRules): void {
  mkdirSync(DATA_DIR, { recursive: true })
  writeFileSync(CUSTOM_RULES_FILE, JSON.stringify(rules, null, 2))
}

async function applyDomainRulesUpdate(prev: DomainRule[], next: DomainRule[]): Promise<{ success: boolean; needsRestart: boolean }> {
  syncCustomDomainRuleSets(next)
  const needsRestart = domainRulesNeedRestart(prev, next)
  const dynamicChanged = serializeDynamicDomainRules(prev) !== serializeDynamicDomainRules(next)
  const service = getServiceInstance()

  if (!needsRestart && dynamicChanged && service?.isRunning()) {
    await service.flushDnsCache()
    await service.closeAllConnections()
  }

  return { success: true, needsRestart }
}

export function initRulesHandlers(): void {
  ipcMain.handle('custom-rules:get', () => {
    return loadCustomRules()
  })

  ipcMain.handle('custom-rules:save', async (_, rules: CustomRules) => {
    const prev = loadCustomRules().domainRules
    saveCustomRules(rules)
    return applyDomainRulesUpdate(prev, rules.domainRules || [])
  })

  ipcMain.handle('domain-rules:get', () => {
    return loadCustomRules().domainRules
  })

  ipcMain.handle('domain-rules:save', async (_, rules: DomainRule[]) => {
    const custom = loadCustomRules()
    const prev = custom.domainRules
    custom.domainRules = rules
    saveCustomRules(custom)
    return applyDomainRulesUpdate(prev, rules)
  })
}
