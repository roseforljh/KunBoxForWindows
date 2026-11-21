import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs'
import type { CustomRules, DomainRule } from '../../shared/types'

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

export function initRulesHandlers(): void {
  ipcMain.handle('custom-rules:get', () => {
    return loadCustomRules()
  })

  ipcMain.handle('custom-rules:save', (_, rules: CustomRules) => {
    saveCustomRules(rules)
    return { success: true }
  })

  ipcMain.handle('domain-rules:get', () => {
    return loadCustomRules().domainRules
  })

  ipcMain.handle('domain-rules:save', (_, rules: DomainRule[]) => {
    const custom = loadCustomRules()
    custom.domainRules = rules
    saveCustomRules(custom)
    return { success: true }
  })
}
