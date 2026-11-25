import { load as loadYaml } from 'js-yaml'
import type { SingBoxOutbound } from '../../shared/types'
import { isProxyType } from '../utils/configGenerator'
import { parseShareLink } from './shareLink'

const RESERVED_OUTBOUND_TAGS = ['PROXY', 'auto', 'direct', 'block'] as const

export function dedupeNodeTags(nodes: SingBoxOutbound[]): SingBoxOutbound[] {
  const used = new Set<string>(RESERVED_OUTBOUND_TAGS)
  return nodes.map((node) => {
    let tag = node.tag?.trim() || `${node.type || 'node'}`
    if (used.has(tag)) {
      let idx = 2
      while (used.has(`${tag} (${idx})`)) idx += 1
      tag = `${tag} (${idx})`
    }
    used.add(tag)
    return tag === node.tag ? node : { ...node, tag }
  })
}

/** 过滤掉 selector / urltest / direct / block / dns 等非代理出站，避免混入节点列表。 */
function filterProxyOutbounds(items: unknown[]): SingBoxOutbound[] {
  return items.filter(
    (item): item is SingBoxOutbound =>
      Boolean(item) && typeof item === 'object' && isProxyType((item as SingBoxOutbound).type)
  )
}

function parseClashProxies(proxies: unknown[]): SingBoxOutbound[] {
  const parsed: SingBoxOutbound[] = []
  for (const raw of proxies) {
    if (!raw || typeof raw !== 'object') continue
    const p = raw as Record<string, any>
    const rawType = typeof p.type === 'string' ? p.type.toLowerCase() : ''
    const type = rawType === 'ss' ? 'shadowsocks' : rawType === 'socks5' ? 'socks' : rawType
    const node: SingBoxOutbound = {
      tag: p.name || `${type}-${p.server}:${p.port}`,
      type,
      server: p.server,
      server_port: p.port
    }

    if (rawType === 'vmess' || rawType === 'vless') {
      node.uuid = p.uuid
      node.alter_id = p.alterId || 0
      node.security = p.cipher || 'auto'
      if (p.network) node.transport = { type: p.network, path: p['ws-opts']?.path, headers: p['ws-opts']?.headers }
      if (p.tls) node.tls = { enabled: true, server_name: p.servername || p.sni, insecure: p['skip-cert-verify'] }
    } else if (rawType === 'shadowsocks' || rawType === 'ss') {
      node.method = p.cipher
      node.password = p.password
    } else if (rawType === 'trojan') {
      node.password = p.password
      if (p.sni) node.tls = { enabled: true, server_name: p.sni }
    } else if (rawType === 'hysteria2') {
      node.password = p.password
      if (p.sni) node.tls = { enabled: true, server_name: p.sni }
    }

    if (isProxyType(node.type)) parsed.push(node)
  }
  return parsed
}

export function parseSubscriptionContent(content: string | object): SingBoxOutbound[] {
  if (content && typeof content === 'object') {
    const obj = content as Record<string, unknown>
    if (Array.isArray(obj.proxies)) return parseClashProxies(obj.proxies)
    if (Array.isArray(obj.outbounds)) return filterProxyOutbounds(obj.outbounds)
    if (Array.isArray(content)) return filterProxyOutbounds(content)
    return []
  }

  const str = String(content ?? '').trim()
  if (!str) return []

  if (str.startsWith('{') || str.startsWith('[')) {
    try {
      const json = JSON.parse(str)
      if (Array.isArray(json)) return filterProxyOutbounds(json)
      if (json && typeof json === 'object') {
        if (Array.isArray(json.outbounds)) return filterProxyOutbounds(json.outbounds)
        if (Array.isArray(json.proxies)) return parseClashProxies(json.proxies)
      }
    } catch {
      // 不是合法 JSON，继续按 YAML 或分享链接解析
    }
  }

  try {
    const yamlDoc = loadYaml(str) as Record<string, unknown> | null
    if (yamlDoc && Array.isArray(yamlDoc.proxies)) {
      return parseClashProxies(yamlDoc.proxies)
    }
  } catch {
    // 不是合法 YAML，继续按 Base64 / 分享链接解析
  }

  let decoded: string
  try {
    decoded = Buffer.from(str.replace(/\s+/g, ''), 'base64').toString('utf-8')
  } catch {
    decoded = str
  }

  const lines = decoded.split(/[\r\n]+/).map(l => l.trim()).filter(Boolean)
  const nodes: SingBoxOutbound[] = []
  for (const line of lines) {
    const parsed = parseShareLink(line)
    if (parsed && isProxyType(parsed.type)) nodes.push(parsed)
  }
  return nodes
}

function hasLocalSettings(node: SingBoxOutbound): boolean {
  if (typeof node.detour === 'string' && node.detour.trim()) return true
  if (typeof node.tcp_fast_open === 'boolean') return true
  return Object.entries(node).some(([key, value]) => key.startsWith('x_kunbox_') && value !== undefined)
}

function applyLocalSettings(prev: SingBoxOutbound, next: SingBoxOutbound): SingBoxOutbound {
  let merged: SingBoxOutbound | null = null
  const setField = (key: string, value: unknown) => {
    if (!merged) merged = { ...next }
    merged[key] = value
  }

  if (typeof prev.detour === 'string' && prev.detour.trim()) {
    setField('detour', prev.detour)
  }

  for (const [key, value] of Object.entries(prev)) {
    if (key.startsWith('x_kunbox_') && value !== undefined) {
      setField(key, value)
    }
  }

  if (typeof prev.tcp_fast_open === 'boolean' && next.tcp_fast_open === undefined) {
    setField('tcp_fast_open', prev.tcp_fast_open)
  }

  return merged ?? next
}

function nodeFingerprint(node: SingBoxOutbound): string | null {
  const type = node.type?.trim().toLowerCase()
  const server = node.server?.trim().toLowerCase()
  if (!type || !server) return null
  const port = node.server_port ?? (Array.isArray(node.server_ports) ? node.server_ports.join(',') : '')
  const credential = node.uuid || node.password || node.username || ''
  return `${type}|${server}|${port}|${credential}`
}

/**
 * 订阅更新时合并用户本地修改的节点配置（前置代理 detour、x_kunbox_* 策略、tcp_fast_open）。
 * 优先按去重后的 tag 精确匹配；若节点改名，仅在服务器+端口+凭据指纹双向唯一时回退继承。
 */
export function mergePreservedNodeSettings(
  existingNodes: SingBoxOutbound[],
  incomingNodes: SingBoxOutbound[]
): SingBoxOutbound[] {
  const dedupedIncoming = dedupeNodeTags(incomingNodes)
  if (!existingNodes.length || !dedupedIncoming.length) return dedupedIncoming

  const existingByTag = new Map<string, SingBoxOutbound>()
  for (const prev of existingNodes) {
    if (prev.tag && !existingByTag.has(prev.tag)) {
      existingByTag.set(prev.tag, prev)
    }
  }

  const matchedPrev = new Set<SingBoxOutbound>()
  const matchedIncomingIdx = new Set<number>()
  const result = dedupedIncoming.map((node, index) => {
    const prev = node.tag ? existingByTag.get(node.tag) : undefined
    if (!prev) return node
    matchedPrev.add(prev)
    matchedIncomingIdx.add(index)
    return applyLocalSettings(prev, node)
  })

  const unmatchedPrevWithSettings = existingNodes.filter(
    (prev) => !matchedPrev.has(prev) && hasLocalSettings(prev)
  )
  if (!unmatchedPrevWithSettings.length) return result

  const prevByFp = new Map<string, SingBoxOutbound | null>()
  for (const prev of unmatchedPrevWithSettings) {
    const fp = nodeFingerprint(prev)
    if (!fp) continue
    prevByFp.set(fp, prevByFp.has(fp) ? null : prev)
  }

  const incomingIdxByFp = new Map<string, number | null>()
  for (let i = 0; i < result.length; i += 1) {
    if (matchedIncomingIdx.has(i)) continue
    const fp = nodeFingerprint(result[i])
    if (!fp) continue
    incomingIdxByFp.set(fp, incomingIdxByFp.has(fp) ? null : i)
  }

  for (const [fp, prev] of prevByFp.entries()) {
    if (!prev) continue
    const targetIdx = incomingIdxByFp.get(fp)
    if (typeof targetIdx === 'number') {
      matchedPrev.add(prev)
      result[targetIdx] = applyLocalSettings(prev, result[targetIdx])
    }
  }

  // 保留用户单独手动添加进该订阅的节点，避免订阅更新覆盖丢失并导致分流规则失效
  const preservedManualNodes = existingNodes.filter(
    (prev) => prev.x_kunbox_manual === true && !matchedPrev.has(prev)
  )
  if (preservedManualNodes.length > 0) {
    return dedupeNodeTags([...result, ...preservedManualNodes])
  }

  return result
}
