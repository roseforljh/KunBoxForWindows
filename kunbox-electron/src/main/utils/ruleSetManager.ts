import { join } from 'path'
import { existsSync, mkdirSync, writeFileSync, readFileSync } from 'fs'
import log from 'electron-log'
import https from 'https'
import http from 'http'
import type { RuleSet } from '../../shared/types'

const DATA_DIR = join(process.env.APPDATA || '', 'KunBox')
const RULESETS_DIR = join(DATA_DIR, 'rulesets')

export interface RuleSetItem extends RuleSet {
  outboundValue?: string
}

function ensureDir(): void {
  if (!existsSync(RULESETS_DIR)) {
    mkdirSync(RULESETS_DIR, { recursive: true })
  }
}

export function getRuleSetLocalPath(tag: string): string {
  return join(RULESETS_DIR, `${tag}.srs`)
}

export function isRuleSetLocal(tag: string): boolean {
  return existsSync(getRuleSetLocalPath(tag))
}

function normalizeUrl(url: string): string {
  if (url.includes('ghp.ci/https://raw.githubusercontent.com/')) {
    return url.replace('https://ghp.ci/', '')
  }
  return url
}

function downloadFile(url: string, timeout = 30000): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const protocol = url.startsWith('https') ? https : http

    const req = protocol.get(url, { timeout }, (res) => {
      if (res.statusCode === 301 || res.statusCode === 302) {
        const location = res.headers.location
        if (location) {
          downloadFile(location, timeout).then(resolve).catch(reject)
          return
        }
      }

      if (res.statusCode !== 200) {
        reject(new Error(`HTTP ${res.statusCode}`))
        return
      }

      const chunks: Buffer[] = []
      res.on('data', (chunk) => chunks.push(chunk))
      res.on('end', () => resolve(Buffer.concat(chunks)))
      res.on('error', reject)
    })

    req.on('error', reject)
    req.on('timeout', () => {
      req.destroy()
      reject(new Error('Request timeout'))
    })
  })
}

export async function downloadRuleSet(ruleSet: RuleSetItem): Promise<boolean> {
  if (ruleSet.type !== 'remote' || !ruleSet.url) {
    return false
  }

  ensureDir()
  const localPath = getRuleSetLocalPath(ruleSet.tag)
  const mirrors = [normalizeUrl(ruleSet.url)]

  for (const url of mirrors) {
    try {
      log.info(`[RuleSet] Downloading ${ruleSet.tag} from ${url}`)
      const data = await downloadFile(url)

      const header = data.slice(0, 64).toString('utf-8')
      if (header.includes('<!DOCTYPE') || header.includes('<html')) {
        log.warn(`[RuleSet] ${ruleSet.tag}: Got HTML instead of binary`)
        continue
      }

      if (data.length < 100) {
        log.warn(`[RuleSet] ${ruleSet.tag}: File too small (${data.length} bytes)`)
        continue
      }

      writeFileSync(localPath, data)
      log.info(`[RuleSet] Downloaded ${ruleSet.tag} (${data.length} bytes)`)
      return true
    } catch (error) {
      log.warn(`[RuleSet] Failed to download ${ruleSet.tag} from ${url}: ${error}`)
    }
  }

  return false
}

export async function downloadAllRuleSets(
  ruleSets: RuleSetItem[],
  onProgress?: (tag: string, current: number, total: number) => void
): Promise<{ success: number; failed: number }> {
  ensureDir()

  const remoteRuleSets = ruleSets.filter(r => r.enabled && r.type === 'remote')
  let success = 0
  let failed = 0

  for (let i = 0; i < remoteRuleSets.length; i++) {
    const ruleSet = remoteRuleSets[i]
    onProgress?.(ruleSet.tag, i + 1, remoteRuleSets.length)

    if (isRuleSetLocal(ruleSet.tag)) {
      log.info(`[RuleSet] ${ruleSet.tag} already cached locally`)
      success++
      continue
    }

    const ok = await downloadRuleSet(ruleSet)
    if (ok) {
      success++
    } else {
      failed++
    }
  }

  return { success, failed }
}

export function checkRuleSetsReady(ruleSets: RuleSetItem[]): {
  ready: RuleSetItem[]
  missing: RuleSetItem[]
} {
  const ready: RuleSetItem[] = []
  const missing: RuleSetItem[] = []

  for (const ruleSet of ruleSets.filter(r => r.enabled)) {
    if (ruleSet.type === 'local') {
      const p = ruleSet.path || ruleSet.url
      if (p && existsSync(p)) {
        ready.push(ruleSet)
      } else {
        missing.push(ruleSet)
      }
    } else {
      if (isRuleSetLocal(ruleSet.tag)) {
        ready.push(ruleSet)
      } else {
        missing.push(ruleSet)
      }
    }
  }

  return { ready, missing }
}

export function loadRuleSetsFromFile(): RuleSetItem[] {
  const rulesetsFile = join(DATA_DIR, 'rulesets.json')
  try {
    if (existsSync(rulesetsFile)) {
      return JSON.parse(readFileSync(rulesetsFile, 'utf-8'))
    }
  } catch (error) {
    log.error('[RuleSet] Failed to load rulesets:', error)
  }
  return []
}
