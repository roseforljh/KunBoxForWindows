import type { OutboundTransport, SingBoxOutbound } from '../../shared/types'

/** tcp 以外的传输方式映射到 sing-box transport；链接里没写就当 tcp。 */
function parseTransport(params: URLSearchParams): OutboundTransport | undefined {
  const type = params.get('type')
  if (!type || type === 'tcp') return undefined
  const transport: OutboundTransport = { type }
  const path = params.get('path')
  if (path) transport.path = path
  const host = params.get('host')
  if (host) transport.headers = { Host: host }
  const serviceName = params.get('serviceName')
  if (serviceName) transport.service_name = serviceName
  const mode = params.get('mode')
  if (mode) transport.mode = mode
  return transport
}

/**
 * 把分享链接解析成 sing-box outbound。
 * reality 的 pbk/sid、uTLS 的 fp、vless 的 flow 缺一个都连不上（表现是握手后卡死/超时），别省。
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
        uuid: url.username
      }
      const flow = params.get('flow')
      if (flow) node.flow = flow

      const security = params.get('security')
      if (security === 'tls' || security === 'reality') {
        node.tls = { enabled: true, server_name: params.get('sni') || url.hostname }
        const fp = params.get('fp')
        if (fp) node.tls.utls = { enabled: true, fingerprint: fp }
        if (security === 'reality') {
          node.tls.reality = { enabled: true }
          const pbk = params.get('pbk')
          if (pbk) node.tls.reality.public_key = pbk
          const sid = params.get('sid')
          if (sid) node.tls.reality.short_id = sid
          // REALITY 必须用 uTLS 发 ClientHello，链接没带 fp 时用 chrome 兜底
          if (!node.tls.utls) node.tls.utls = { enabled: true, fingerprint: 'chrome' }
        }
      }

      const transport = parseTransport(params)
      if (transport) node.transport = transport
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
