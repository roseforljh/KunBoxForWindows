import { join } from 'path'
import { existsSync, mkdirSync, writeFileSync, readFileSync } from 'fs'
import log from 'electron-log'
import https from 'https'
import http from 'http'
import type { DomainRule, RuleSet } from '../../shared/types'

import { getDataDir } from './paths'

const DATA_DIR = getDataDir()
const RULESETS_DIR = join(DATA_DIR, 'rulesets')

// 动态本地 source 规则集标签（内核通过 fswatch 自动监听热重载）
export const DYNAMIC_RULE_SET_TAGS = {
  bypassLan: '_kunbox_bypass_lan',
  customDirect: '_kunbox_custom_direct',
  customProxy: '_kunbox_custom_proxy',
  customBlock: '_kunbox_custom_block'
} as const

// 绕过局域网使用的私网 CIDR（排除 198.18.0.0/15 避免误直连 FakeIP）
export const PRIVATE_IP_CIDRS = [
  '10.0.0.0/8',
  '100.64.0.0/10',
  '127.0.0.0/8',
  '169.254.0.0/16',
  '172.16.0.0/12',
  '192.168.0.0/16',
  '224.0.0.0/4',
  '255.255.255.255/32',
  '::1/128',
  'fc00::/7',
  'fe80::/10',
  'ff00::/8'
]

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

export function getDynamicRuleSetPath(tag: string): string {
  return join(RULESETS_DIR, `${tag}.json`)
}

function writeSourceRuleSet(tag: string, rules: object[]): string {
  ensureDir()
  const filePath = getDynamicRuleSetPath(tag)
  const payload = JSON.stringify({ version: 2, rules }, null, 2)
  writeFileSync(filePath, payload, 'utf-8')
  return filePath
}

// 同步「绕过局域网」本地规则集文件，RayBox fswatch 会自动热加载变更
export function syncBypassLanRuleSet(enabled: boolean): string {
  return writeSourceRuleSet(
    DYNAMIC_RULE_SET_TAGS.bypassLan,
    enabled ? [{ ip_cidr: PRIVATE_IP_CIDRS }] : []
  )
}

function buildDomainRuleEntries(rules: DomainRule[]): object[] {
  const domain: string[] = []
  const domainSuffix: string[] = []
  const domainKeyword: string[] = []
  for (const rule of rules) {
    if (!rule.enabled || !rule.value?.trim()) continue
    const value = rule.value.trim()
    if (rule.type === 'domain') domain.push(value)
    else if (rule.type === 'domain_suffix') domainSuffix.push(value)
    else if (rule.type === 'domain_keyword') domainKeyword.push(value)
  }
  const entry: Record<string, string[]> = {}
  if (domain.length > 0) entry.domain = domain
  if (domainSuffix.length > 0) entry.domain_suffix = domainSuffix
  if (domainKeyword.length > 0) entry.domain_keyword = domainKeyword
  return Object.keys(entry).length > 0 ? [entry] : []
}

// 同步自定义域名规则（direct / proxy / block）到本地规则集文件以支持热重载
export function syncCustomDomainRuleSets(domainRules: DomainRule[]): Record<'direct' | 'proxy' | 'block', string> {
  const directRules = buildDomainRuleEntries(domainRules.filter((r) => r.outboundMode === 'direct'))
  const proxyRules = buildDomainRuleEntries(domainRules.filter((r) => r.outboundMode === 'proxy'))
  const blockRules = buildDomainRuleEntries(domainRules.filter((r) => r.outboundMode === 'block'))

  return {
    direct: writeSourceRuleSet(DYNAMIC_RULE_SET_TAGS.customDirect, directRules),
    proxy: writeSourceRuleSet(DYNAMIC_RULE_SET_TAGS.customProxy, proxyRules),
    block: writeSourceRuleSet(DYNAMIC_RULE_SET_TAGS.customBlock, blockRules)
  }
}

export function serializeDynamicDomainRules(rules: DomainRule[]): string {
  return JSON.stringify(
    rules
      .filter((r) => r.enabled && (r.outboundMode === 'direct' || r.outboundMode === 'proxy' || r.outboundMode === 'block'))
      .map((r) => [r.type, r.value.trim(), r.outboundMode])
  )
}

// 仅当指定节点 / 订阅组的启用域名规则发生变化时才需要重启内核；其余 direct/proxy/block 走本地规则集热载
export function domainRulesNeedRestart(prev: DomainRule[], next: DomainRule[]): boolean {
  const serializeStatic = (list: DomainRule[]) =>
    JSON.stringify(
      list
        .filter((r) => r.enabled && (r.outboundMode === 'node' || r.outboundMode === 'profile'))
        .map((r) => [r.type, r.value.trim(), r.outboundMode, r.outboundValue || ''])
    )
  return serializeStatic(prev) !== serializeStatic(next)
}

// 仅当实际进入 config.json 的启用规则集（成员、顺序、路径或出站绑定）变化时才需要重启
export function ruleSetsNeedRestart(prev: RuleSetItem[], next: RuleSetItem[]): boolean {
  const serializeActive = (list: RuleSetItem[]) => {
    const ordered = [
      ...list.filter((rs) => !rs.isBuiltIn),
      ...list.filter((rs) => rs.isBuiltIn)
    ]
    return JSON.stringify(
      ordered
        .filter((r) => r.enabled && (r.type !== 'remote' || isRuleSetLocal(r.tag)))
        .map((r) => [
          r.tag,
          r.type,
          r.format,
          r.type === 'local' ? (r.path || r.url || '') : '',
          r.outboundMode,
          r.outboundValue || ''
        ])
    )
  }
  return serializeActive(prev) !== serializeActive(next)
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
