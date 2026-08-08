import { buildManualNodeLink, createManualNodeDraft } from './manual-node'
import type { ManualNodeProtocol } from './manual-node'

function assertEqual(actual: string, expected: string, message: string) {
  if (actual !== expected) throw new Error(`${message}: ${actual}`)
}

const socks = createManualNodeDraft('socks5')
socks.name = 'SOCKS 节点'
socks.server = 'proxy.example.com'
socks.username = 'demo user'
socks.password = 'p@ss:word'
assertEqual(
  buildManualNodeLink(socks),
  'socks5://demo%20user:p%40ss%3Aword@proxy.example.com:1080#SOCKS%20%E8%8A%82%E7%82%B9',
  'SOCKS5 链接生成错误',
)

const https = createManualNodeDraft('http')
https.name = 'HTTPS 节点'
https.server = 'proxy.example.com'
https.tlsMode = 'tls'
https.serverName = 'tls.example.com'
https.allowInsecure = true
assertEqual(
  buildManualNodeLink(https),
  'https://proxy.example.com:8080?sni=tls.example.com&insecure=1#HTTPS%20%E8%8A%82%E7%82%B9',
  'HTTPS TLS 参数生成错误',
)

const vmess = createManualNodeDraft('vmess')
vmess.name = 'VMess 节点'
vmess.server = 'vmess.example.com'
vmess.uuid = '11111111-1111-1111-1111-111111111111'
vmess.allowInsecure = true
const vmessLink = buildManualNodeLink(vmess)
const vmessPayload = JSON.parse(Buffer.from(vmessLink.slice('vmess://'.length), 'base64').toString('utf8'))
if (vmessPayload.allowInsecure !== true) {
  throw new Error(`VMess 跳过证书验证参数缺失: ${vmessLink}`)
}

const vless = createManualNodeDraft('vless')
vless.name = 'Reality'
vless.server = 'vless.example.com'
vless.uuid = '11111111-1111-1111-1111-111111111111'
vless.tlsMode = 'reality'
vless.publicKey = 'public-key'
vless.shortId = 'abcd'
const vlessLink = buildManualNodeLink(vless)
if (!vlessLink.includes('security=reality') || !vlessLink.includes('pbk=public-key')) {
  throw new Error(`VLESS Reality 参数缺失: ${vlessLink}`)
}

const invalid = createManualNodeDraft('trojan')
invalid.name = 'Invalid'
invalid.server = 'example.com'
let invalidRejected = false
try {
  buildManualNodeLink(invalid)
} catch {
  invalidRejected = true
}
if (!invalidRejected) throw new Error('缺少密码的 Trojan 节点未被拒绝')

const allProtocols: ManualNodeProtocol[] = [
  'socks5', 'http', 'shadowsocks', 'vmess', 'vless', 'trojan',
  'hysteria2', 'hysteria', 'tuic', 'anytls', 'naive',
]
allProtocols.forEach((protocol) => {
  const draft = createManualNodeDraft(protocol)
  draft.name = `${protocol} node`
  draft.server = 'node.example.com'
  draft.username = 'user'
  draft.password = 'password'
  draft.uuid = '11111111-1111-1111-1111-111111111111'
  draft.auth = 'auth'
  const link = buildManualNodeLink(draft)
  if (!link) throw new Error(`${protocol} 未生成节点链接`)
})
