import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { parseShareLink } from './shareLink'

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
