import type { OutboundTransport, SingBoxOutbound } from '../../shared/types'

/** tcp 以外的传输方式映射到 sing-box transport；链接里没写就当 tcp。 */
function parseTransport(params: URLSearchParams): OutboundTransport | undefined {
  const rawType = params.get('type')?.trim().toLowerCase()
  if (!rawType || rawType === 'tcp') return undefined
  const type = rawType === 'h2' ? 'http' : rawType
  const transport: OutboundTransport = { type }
  const path = params.get('path')
  if (path) transport.path = path
  const host = params.get('host')?.trim()
  if (host) {
    if (type === 'xhttp' || type === 'splithttp' || type === 'httpupgrade') {
      transport.host = host
    } else if (type === 'http') {
      const hosts = host.split(',').map((item) => item.trim()).filter(Boolean)
      if (hosts.length) transport.host = hosts
    } else {
      transport.headers = { Host: host }
    }
  }
  const serviceName = params.get('serviceName')
  if (serviceName) transport.service_name = serviceName
  const mode = params.get('mode')?.trim()
  if (mode) transport.mode = mode
  const extraRaw = params.get('extra')?.trim()
  if (extraRaw) {
    try {
      const parsed = JSON.parse(extraRaw)
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        transport.extra = parsed as Record<string, unknown>
      } else {
        transport.extra = extraRaw
      }
    } catch {
      transport.extra = extraRaw
    }
  }
  return transport
}

/**
 * 把分享链接解析成 sing-box outbound。
 * reality 的 pbk/sid、uTLS 的 fp、vless 的 flow/encryption 缺一个都连不上（表现是握手后卡死/断开），别省。
 */
export function parseShareLink(link: string): SingBoxOutbound | null {
  try {
    const url = new URL(link)
    const protocol = url.protocol.replace(':', '').toLowerCase()
    const tag = decodeURIComponent(url.hash ? url.hash.slice(1) : `${protocol}-${url.hostname}`)
    const params = url.searchParams

    if (protocol === 'vless' || protocol === 'vmess') {
      const node: SingBoxOutbound = {
        tag,
        type: protocol,
        server: url.hostname,
        server_port: parseInt(url.port) || 443,
        uuid: decodeURIComponent(url.username)
      }
      const flow = params.get('flow')?.trim()
      if (flow) node.flow = flow

      if (protocol === 'vless') {
        const encryption = params.get('encryption')?.trim()
        if (encryption && encryption.toLowerCase() !== 'none') {
          node.encryption = encryption
        }
      }

      const rawSecurity = params.get('security')?.trim().toLowerCase() || ''
      const hasTlsHint = ['sni', 'servername', 'alpn', 'fp', 'insecure', 'allowInsecure'].some((key) => params.has(key))
      const security = rawSecurity || (hasTlsHint ? 'tls' : '')
      if (security === 'tls' || security === 'reality') {
        node.tls = { enabled: true, server_name: params.get('sni') || params.get('servername') || url.hostname }
        const fp = params.get('fp')?.trim()
        if (fp) node.tls.utls = { enabled: true, fingerprint: fp }
        if (security === 'reality') {
          node.tls.reality = { enabled: true }
          const pbk = params.get('pbk')?.trim()
          if (pbk) node.tls.reality.public_key = pbk
          const sid = params.get('sid')?.trim()
          if (sid) node.tls.reality.short_id = sid
          // REALITY 必须用 uTLS 发 ClientHello，链接没带 fp 时用 chrome 兜底
          if (!node.tls.utls) node.tls.utls = { enabled: true, fingerprint: 'chrome' }
        } else {
          const insecureRaw = (params.get('insecure') || params.get('allowInsecure') || '').trim().toLowerCase()
          if (insecureRaw === '1' || insecureRaw === 'true') {
            node.tls.insecure = true
          }
          const alpnRaw = params.get('alpn')?.trim()
          if (alpnRaw) {
            const alpn = alpnRaw.split(',').map((item) => item.trim()).filter(Boolean)
            if (alpn.length) node.tls.alpn = alpn
          }
        }
      }

      const transport = parseTransport(params)
      if (transport) {
        // 兼容旧格式将 VLESS encryption 写在 xhttp extra JSON 内的写法，提升到顶层 encryption
        if (
          protocol === 'vless' &&
          transport.extra &&
          typeof transport.extra === 'object' &&
          !Array.isArray(transport.extra) &&
          typeof transport.extra.encryption === 'string'
        ) {
          const extraEnc = transport.extra.encryption.trim()
          if (!node.encryption && extraEnc && extraEnc.toLowerCase() !== 'none') {
            node.encryption = extraEnc
          }
          const restExtra = { ...transport.extra }
          delete restExtra.encryption
          if (Object.keys(restExtra).length > 0) {
            transport.extra = restExtra
          } else {
            delete transport.extra
          }
        }
        node.transport = transport
      }
      return node
    }

    if (protocol === 'trojan') {
      return {
        tag,
        type: 'trojan',
        server: url.hostname,
        server_port: parseInt(url.port) || 443,
        password: url.username,
        tls: { enabled: true, server_name: params.get('sni') || url.hostname }
      }
    }

    if (protocol === 'ss') {
      let method = 'aes-256-gcm'
      let password = ''
      if (url.username) {
        try {
          const cred = Buffer.from(url.username, 'base64').toString('utf-8')
          const parts = cred.split(':')
          method = parts[0]
          password = parts[1] || ''
        } catch {
          password = url.username
        }
      }
      return {
        tag,
        type: 'shadowsocks',
        server: url.hostname,
        server_port: parseInt(url.port) || 8388,
        method,
        password
      }
    }

    if (protocol === 'hysteria2' || protocol === 'hy2') {
      return {
        tag,
        type: 'hysteria2',
        server: url.hostname,
        server_port: parseInt(url.port) || 443,
        password: url.username,
        tls: { enabled: true, server_name: params.get('sni') || url.hostname }
      }
    }
  } catch {
    // not valid url
  }
  return null
}

function formatHost(server: string): string {
  const host = server.trim()
  return host.includes(':') && !(host.startsWith('[') && host.endsWith(']')) ? `[${host}]` : host
}

function buildQuery(entries: Array<[string, string | undefined]>): string {
  const params = new URLSearchParams()
  for (const [key, value] of entries) {
    if (value !== undefined && value !== '') params.set(key, value)
  }
  const query = params.toString()
  return query ? `?${query}` : ''
}

function extractTransportHost(transport?: OutboundTransport): string | undefined {
  if (!transport) return undefined
  const headerHost = transport.headers?.Host ?? transport.headers?.host
  if (Array.isArray(headerHost)) return headerHost[0]
  if (typeof headerHost === 'string' && headerHost) return headerHost
  if (Array.isArray(transport.host)) return transport.host[0]
  if (typeof transport.host === 'string' && transport.host) return transport.host
  return undefined
}

function buildTransportQuery(transport?: OutboundTransport): Array<[string, string | undefined]> {
  const type = transport?.type || 'tcp'
  const entries: Array<[string, string | undefined]> = [['type', type]]
  if (!transport || type === 'tcp') return entries
  if (transport.path) entries.push(['path', transport.path])
  const host = extractTransportHost(transport)
  if (host) entries.push(['host', host])
  if (transport.service_name) entries.push(['serviceName', transport.service_name])
  if (transport.mode) entries.push(['mode', transport.mode])
  if (transport.extra !== undefined) {
    const extraVal = typeof transport.extra === 'string'
      ? transport.extra
      : Object.keys(transport.extra).length > 0
        ? JSON.stringify(transport.extra)
        : undefined
    if (extraVal) entries.push(['extra', extraVal])
  }
  return entries
}

function buildUserInfo(username?: string, password?: string): string {
  if (!username && !password) return ''
  return `${encodeURIComponent(username || '')}:${encodeURIComponent(password || '')}@`
}

/** 将 sing-box outbound 节点序列化为标准分享链接，自动忽略 detour / x_* / 运行时状态字段。 */
export function serializeShareLink(node: SingBoxOutbound): string {
  const type = (node.type || '').toLowerCase()
  const server = typeof node.server === 'string' ? node.server.trim() : ''
  const port = node.server_port
  if (!server || !port) {
    throw new Error(`节点缺少有效的服务器地址或端口，无法导出链接`)
  }

  const host = formatHost(server)
  const tagName = (node.tag || `${type}-${server}`).trim()
  const tag = encodeURIComponent(tagName)
  const tls = node.tls
  const alpn = tls?.alpn?.length ? tls.alpn.join(',') : undefined

  switch (type) {
    case 'vless': {
      const security = tls?.enabled ? (tls.reality?.enabled ? 'reality' : 'tls') : 'none'
      const query = buildQuery([
        ['encryption', node.encryption || 'none'],
        ['flow', node.flow || undefined],
        ['security', security],
        ...buildTransportQuery(node.transport),
        ['sni', tls?.enabled ? (tls.server_name || undefined) : undefined],
        ['fp', tls?.enabled ? (tls.utls?.fingerprint || undefined) : undefined],
        ['pbk', security === 'reality' ? (tls?.reality?.public_key || undefined) : undefined],
        ['sid', security === 'reality' ? (tls?.reality?.short_id || undefined) : undefined],
        ['alpn', tls?.enabled ? alpn : undefined],
        ['insecure', tls?.enabled && tls.insecure ? '1' : undefined],
      ])
      return `vless://${encodeURIComponent(node.uuid || '')}@${host}:${port}${query}#${tag}`
    }

    case 'vmess': {
      const transport = node.transport
      const payload: Record<string, unknown> = {
        v: '2',
        ps: tagName,
        add: server,
        port: String(port),
        id: node.uuid || '',
        aid: String(node.alter_id ?? 0),
        scy: node.security || 'auto',
        net: transport?.type || 'tcp',
        type: 'none',
        host: extractTransportHost(transport) || '',
        path: transport?.type === 'grpc' ? (transport.service_name || '') : (transport?.path || ''),
        tls: tls?.enabled ? 'tls' : '',
        sni: tls?.enabled ? (tls.server_name || '') : '',
        alpn: tls?.enabled ? (alpn || '') : '',
        fp: tls?.enabled ? (tls.utls?.fingerprint || '') : '',
      }
      if (tls?.enabled && tls.insecure) payload.allowInsecure = true
      return `vmess://${Buffer.from(JSON.stringify(payload), 'utf-8').toString('base64')}`
    }

    case 'trojan': {
      const security = tls?.reality?.enabled ? 'reality' : 'tls'
      const query = buildQuery([
        ['security', security],
        ...buildTransportQuery(node.transport),
        ['sni', tls?.server_name || undefined],
        ['fp', tls?.utls?.fingerprint || undefined],
        ['pbk', security === 'reality' ? (tls?.reality?.public_key || undefined) : undefined],
        ['sid', security === 'reality' ? (tls?.reality?.short_id || undefined) : undefined],
        ['alpn', alpn],
        ['allowInsecure', tls?.insecure ? '1' : undefined],
      ])
      return `trojan://${encodeURIComponent(node.password || '')}@${host}:${port}${query}#${tag}`
    }

    case 'shadowsocks':
    case 'ss': {
      const userInfo = Buffer.from(`${node.method || 'aes-256-gcm'}:${node.password || ''}`, 'utf-8').toString('base64')
      const pluginValue = node.plugin
        ? (node.plugin_opts ? `${node.plugin};${node.plugin_opts}` : node.plugin)
        : undefined
      const query = buildQuery([['plugin', pluginValue]])
      return `ss://${userInfo}@${host}:${port}${query}#${tag}`
    }

    case 'hysteria2':
    case 'hy2': {
      const obfsObj = typeof node.obfs === 'object' && node.obfs !== null ? node.obfs : undefined
      const mport = Array.isArray(node.server_ports) && node.server_ports.length > 0
        ? node.server_ports.map((item) => String(item).replace(':', '-')).join(',')
        : undefined
      const query = buildQuery([
        ['mport', mport],
        ['sni', tls?.server_name || undefined],
        ['insecure', tls?.insecure ? '1' : undefined],
        ['alpn', alpn],
        ['obfs', obfsObj?.type || undefined],
        ['obfs-password', obfsObj?.password || undefined],
      ])
      return `hysteria2://${encodeURIComponent(node.password || '')}@${host}:${port}${query}#${tag}`
    }

    case 'hysteria': {
      const query = buildQuery([
        ['auth', node.auth_str || undefined],
        ['peer', tls?.server_name || undefined],
        ['insecure', tls?.insecure ? '1' : undefined],
        ['upmbps', node.up_mbps !== undefined ? String(node.up_mbps) : undefined],
        ['downmbps', node.down_mbps !== undefined ? String(node.down_mbps) : undefined],
        ['obfs', typeof node.obfs === 'string' ? node.obfs : undefined],
        ['alpn', alpn],
      ])
      return `hysteria://${host}:${port}${query}#${tag}`
    }

    case 'tuic': {
      const query = buildQuery([
        ['sni', tls?.server_name || undefined],
        ['allow_insecure', tls?.insecure ? '1' : undefined],
        ['congestion_control', node.congestion_control || undefined],
        ['udp_relay_mode', node.udp_relay_mode || undefined],
        ['alpn', alpn],
      ])
      return `tuic://${encodeURIComponent(node.uuid || '')}:${encodeURIComponent(node.password || '')}@${host}:${port}${query}#${tag}`
    }

    case 'anytls': {
      const query = buildQuery([
        ['sni', tls?.server_name || undefined],
        ['insecure', tls?.insecure ? '1' : undefined],
        ['fp', tls?.utls?.fingerprint || undefined],
        ['alpn', alpn],
      ])
      return `anytls://${encodeURIComponent(node.password || '')}@${host}:${port}${query}#${tag}`
    }

    case 'naive': {
      const scheme = node.quic ? 'naive+quic' : 'naive+https'
      const query = buildQuery([
        ['sni', tls?.server_name || undefined],
        ['insecure', tls?.insecure ? '1' : undefined],
      ])
      return `${scheme}://${buildUserInfo(node.username, node.password)}${host}:${port}${query}#${tag}`
    }

    case 'socks':
    case 'socks5':
      return `socks5://${buildUserInfo(node.username, node.password)}${host}:${port}#${tag}`

    case 'http': {
      const scheme = tls?.enabled ? 'https' : 'http'
      const query = buildQuery([
        ['sni', tls?.enabled ? (tls.server_name || undefined) : undefined],
        ['insecure', tls?.enabled && tls.insecure ? '1' : undefined],
      ])
      return `${scheme}://${buildUserInfo(node.username, node.password)}${host}:${port}${query}#${tag}`
    }

    default:
      throw new Error(`暂不支持将 ${type || '未知'} 类型节点导出为分享链接`)
  }
}
