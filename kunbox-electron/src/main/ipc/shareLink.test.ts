import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { parseShareLink, serializeShareLink } from './shareLink'

const realityLink =
  'vless://3a32d9dd-bfe5-40a8-8e2e-b51633b0c976@46.149.227.209:18443?encryption=none&flow=xtls-rprx-vision&security=reality&type=tcp&sni=itunes.apple.com&fp=firefox&pbk=PifZEYkM2V4snZoksXX82-NjrX4PWvzBodpIdFgl9FE&sid=b6dd397cbf1b02c1#ru2-direct'

describe('parseShareLink', () => {
  it('keeps reality/utls/flow so a REALITY node can actually connect', () => {
    const node = parseShareLink(realityLink)
    assert.ok(node, '链接应解析成功')
    assert.equal(node.type, 'vless')
    assert.equal(node.server, '46.149.227.209')
    assert.equal(node.server_port, 18443)
    assert.equal(node.uuid, '3a32d9dd-bfe5-40a8-8e2e-b51633b0c976')
    assert.equal(node.flow, 'xtls-rprx-vision')
    assert.equal(node.tls?.enabled, true)
    assert.equal(node.tls?.server_name, 'itunes.apple.com')
    assert.equal(node.tls?.utls?.enabled, true)
    assert.equal(node.tls?.utls?.fingerprint, 'firefox')
    assert.equal(node.tls?.reality?.enabled, true)
    assert.equal(node.tls?.reality?.public_key, 'PifZEYkM2V4snZoksXX82-NjrX4PWvzBodpIdFgl9FE')
    assert.equal(node.tls?.reality?.short_id, 'b6dd397cbf1b02c1')
    assert.equal(node.transport, undefined, 'tcp 不应生成 transport')
  })

  it('defaults utls to chrome when a reality link omits fp', () => {
    const node = parseShareLink('vless://11111111-1111-1111-1111-111111111111@a.example.com:443?security=reality&pbk=abc#n')
    assert.equal(node?.tls?.utls?.fingerprint, 'chrome')
  })

  it('maps ws transport', () => {
    const node = parseShareLink('vless://11111111-1111-1111-1111-111111111111@a.example.com:443?type=ws&path=%2Fray&host=cdn.example.com#ws')
    assert.deepEqual(node?.transport, { type: 'ws', path: '/ray', headers: { Host: 'cdn.example.com' } })
  })
})

describe('serializeShareLink', () => {
  it('exports a VLESS REALITY node as a vless:// link that round-trips cleanly', () => {
    const original = parseShareLink(realityLink)
    assert.ok(original)
    const exported = serializeShareLink({
      ...original,
      detour: 'relay-node',
      x_kunbox_auto_selection_eligible: true,
      latencyMs: 193,
    })
    assert.ok(exported.startsWith('vless://'))
    assert.ok(!exported.includes('detour'))
    assert.ok(!exported.includes('latencyMs'))
    assert.deepEqual(parseShareLink(exported), original)
  })

  it('exports VMess as standard base64 vmess:// link', () => {
    const exported = serializeShareLink({
      tag: '香港 VMess',
      type: 'vmess',
      server: 'vm.example.com',
      server_port: 443,
      uuid: '11111111-1111-1111-1111-111111111111',
      alter_id: 0,
      security: 'auto',
      tls: { enabled: true, server_name: 'cdn.example.com', utls: { enabled: true, fingerprint: 'chrome' } },
      transport: { type: 'ws', path: '/ws', headers: { Host: 'cdn.example.com' } },
    })
    assert.ok(exported.startsWith('vmess://'))
    const decoded = JSON.parse(Buffer.from(exported.slice('vmess://'.length), 'base64').toString('utf-8'))
    assert.equal(decoded.ps, '香港 VMess')
    assert.equal(decoded.add, 'vm.example.com')
    assert.equal(decoded.port, '443')
    assert.equal(decoded.id, '11111111-1111-1111-1111-111111111111')
    assert.equal(decoded.net, 'ws')
    assert.equal(decoded.path, '/ws')
    assert.equal(decoded.host, 'cdn.example.com')
    assert.equal(decoded.tls, 'tls')
    assert.equal(decoded.sni, 'cdn.example.com')
  })

  it('exports Shadowsocks, Trojan, Hysteria2, TUIC and wraps IPv6 hosts', () => {
    const ss = serializeShareLink({
      tag: 'SS 节点',
      type: 'shadowsocks',
      server: '2001:db8::1',
      server_port: 8388,
      method: 'aes-256-gcm',
      password: 'secret',
    })
    assert.ok(ss.startsWith('ss://'))
    assert.ok(ss.includes('@[2001:db8::1]:8388#SS%20%E8%8A%82%E7%82%B9'))
    const parsedSs = parseShareLink(ss)
    assert.equal(parsedSs?.method, 'aes-256-gcm')
    assert.equal(parsedSs?.password, 'secret')

    const trojan = serializeShareLink({
      tag: 'TR',
      type: 'trojan',
      server: 'tr.example.com',
      server_port: 443,
      password: 'pass',
      tls: { enabled: true, server_name: 'tr.example.com', insecure: true },
    })
    assert.ok(trojan.startsWith('trojan://pass@tr.example.com:443?'))
    assert.ok(trojan.includes('allowInsecure=1'))

    const hy2 = serializeShareLink({
      tag: 'HY2',
      type: 'hysteria2',
      server: 'hy2.example.com',
      server_port: 443,
      password: 'pwd',
      server_ports: ['20000:30000'],
      obfs: { type: 'salamander', password: 'obfsp' },
      tls: { enabled: true, server_name: 'hy2.example.com' },
    })
    assert.ok(hy2.startsWith('hysteria2://pwd@hy2.example.com:443?'))
    assert.ok(hy2.includes('mport=20000-30000'))
    assert.ok(hy2.includes('obfs=salamander'))
    assert.ok(hy2.includes('obfs-password=obfsp'))

    const tuic = serializeShareLink({
      tag: 'TUIC',
      type: 'tuic',
      server: 'tuic.example.com',
      server_port: 443,
      uuid: 'u1',
      password: 'p1',
      congestion_control: 'bbr',
      udp_relay_mode: 'native',
      tls: { enabled: true, server_name: 'tuic.example.com', alpn: ['h3'] },
    })
    assert.ok(tuic.startsWith('tuic://u1:p1@tuic.example.com:443?'))
    assert.ok(tuic.includes('congestion_control=bbr'))
  })

  it('rejects unsupported outbound types instead of dumping JSON', () => {
    assert.throws(
      () => serializeShareLink({ tag: 'WG', type: 'wireguard', server: '1.2.3.4', server_port: 51820 }),
      /暂不支持将 wireguard 类型节点导出为分享链接/
    )
  })
})
