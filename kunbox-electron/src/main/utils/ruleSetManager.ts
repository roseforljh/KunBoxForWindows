import { join } from 'path'
import { existsSync, mkdirSync, writeFileSync, readFileSync } from 'fs'
import log from 'electron-log'
import https from 'https'
import http from 'http'

const DATA_DIR = join(process.env.APPDATA || '', 'KunBox')
const RULESETS_DIR = join(DATA_DIR, 'rulesets')

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

// Ensure rulesets directory exists
function ensureDir(): void {
  if (!existsSync(RULESETS_DIR)) {
    mkdirSync(RULESETS_DIR, { recursive: true })
  }
}

// Get local path for a ruleset
export function getRuleSetLocalPath(tag: string): string {
  return join(RULESETS_DIR, `${tag}.srs`)
}

// Check if ruleset exists locally
export function isRuleSetLocal(tag: string): boolean {
  return existsSync(getRuleSetLocalPath(tag))
}

// Convert GitHub URL to accessible mirror
function normalizeUrl(url: string): string {
  // ghp.ci -> raw.githubusercontent.com (original)
  if (url.includes('ghp.ci/https://raw.githubusercontent.com/')) {
    return url.replace('https://ghp.ci/', '')
  }
  return url
}

// Download a file with timeout
function downloadFile(url: string, timeout = 30000): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const protocol = url.startsWith('https') ? https : http
    
    const req = protocol.get(url, { timeout }, (res) => {
      // Follow redirects
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

// Download a single ruleset
export async function downloadRuleSet(ruleSet: RuleSetItem): Promise<boolean> {
  if (ruleSet.type !== 'remote' || !ruleSet.url) {
    return false
  }
  
  ensureDir()
  const localPath = getRuleSetLocalPath(ruleSet.tag)
  
  // Try multiple mirrors
  const mirrors = [
    // Original GitHub raw URL
    normalizeUrl(ruleSet.url),
    // Try with proxy if available (will be added later)
  ]
  
  for (const url of mirrors) {
    try {
      log.info(`[RuleSet] Downloading ${ruleSet.tag} from ${url}`)
      const data = await downloadFile(url)
      
      // Validate: should be binary, not HTML
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

// Download all enabled rulesets
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
    
    // Skip if already exists locally
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

// Check which rulesets are ready (have local cache)
export function checkRuleSetsReady(ruleSets: RuleSetItem[]): {
  ready: RuleSetItem[]
  missing: RuleSetItem[]
} {
  const ready: RuleSetItem[] = []
  const missing: RuleSetItem[] = []
  
  for (const ruleSet of ruleSets.filter(r => r.enabled)) {
    if (ruleSet.type === 'local') {
      // Local rulesets check their path
      if (existsSync(ruleSet.url)) { // url field contains path for local type
        ready.push(ruleSet)
      } else {
        missing.push(ruleSet)
      }
    } else {
      // Remote rulesets check local cache
      if (isRuleSetLocal(ruleSet.tag)) {
        ready.push(ruleSet)
      } else {
        missing.push(ruleSet)
      }
    }
  }
  
  return { ready, missing }
}

// Load rulesets from config file
export function loadRuleSets(): RuleSetItem[] {
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
