import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import type { SingBoxOutbound } from '../../shared/types'
import { dedupeNodeTags, mergePreservedNodeSettings, parseSubscriptionContent } from './subscription'

describe('parseSubscriptionContent', () => {
  it('filters out selector, urltest, direct, block, and dns outbounds from sing-box JSON configs', () => {
    const config = {
      outbounds: [
        { type: 'selector', tag: '🌀 节点之门', outbounds: ['✨ 自动神机', 'ru2'] },
        { type: 'urltest', tag: '✨ 自动神机', outbounds: ['ru2'] },
        { type: 'vless', tag: '🇺🇸 美国家宽 1.3u', server: '23.106.45.205', server_port: 23070, uuid: 'u1' },
        { type: 'hysteria2', tag: '🇺🇸 H2 | 狰 Co', server: '107.175.79.28', server_port: 59644, password: 'p1' },
        { type: 'direct', tag: 'direct' },
        { type: 'block', tag: 'block' },
        { type: 'dns', tag: 'dns-out' }
      ]
    }

    const fromObject = parseSubscriptionContent(config)
    const fromJsonString = parseSubscriptionContent(JSON.stringify(config))

    assert.deepEqual(fromObject.map(n => n.tag), ['🇺🇸 美国家宽 1.3u', '🇺🇸 H2 | 狰 Co'])
    assert.deepEqual(fromJsonString.map(n => n.tag), ['🇺🇸 美国家宽 1.3u', '🇺🇸 H2 | 狰 Co'])
  })

  it('filters non-proxy entries from bare outbound arrays', () => {
    const raw = JSON.stringify([
      { type: 'direct', tag: 'direct' },
      { type: 'vless', tag: 'ru2', server: '46.149.227.209', server_port: 9443, uuid: 'u2' },
      { type: 'block', tag: 'block' }
    ])
    const parsed = parseSubscriptionContent(raw)
    assert.deepEqual(parsed.map(n => n.tag), ['ru2'])
  })
})

describe('mergePreservedNodeSettings', () => {
  it('preserves detour, x_kunbox_* policies, and tcp_fast_open on matching tags while accepting remote updates', () => {
    const existing: SingBoxOutbound[] = [
      {
        type: 'vless',
        tag: '🇺🇸 美国家宽 1.3u',
        server: '23.106.45.205',
        server_port: 23070,
        uuid: 'old-uuid',
        detour: 'lxy-profile::香港中转',
        x_kunbox_metered_protected: true,
        x_kunbox_auto_selection_eligible: false,
        tcp_fast_open: true
      }
    ]
    const incoming: SingBoxOutbound[] = [
      {
        type: 'vless',
        tag: '🇺🇸 美国家宽 1.3u',
        server: '23.106.45.205',
        server_port: 24000,
        uuid: 'new-uuid'
      }
    ]

    const merged = mergePreservedNodeSettings(existing, incoming)
    assert.equal(merged.length, 1)
    assert.equal(merged[0].server_port, 24000)
    assert.equal(merged[0].uuid, 'new-uuid')
    assert.equal(merged[0].detour, 'lxy-profile::香港中转')
    assert.equal(merged[0].x_kunbox_metered_protected, true)
    assert.equal(merged[0].x_kunbox_auto_selection_eligible, false)
    assert.equal(merged[0].tcp_fast_open, true)
  })

  it('falls back to unique server+port+credential fingerprint when a node is renamed without confusing shared-IP CDN nodes', () => {
    const existing: SingBoxOutbound[] = [
      {
        type: 'vless',
        tag: '🇩🇪 德国旧名',
        server: '104.25.240.201',
        server_port: 443,
        uuid: 'uuid-de',
        detour: 'profile-a::relay'
      },
      {
        type: 'vless',
        tag: '🇳🇱 荷兰',
        server: '104.25.240.201',
        server_port: 443,
        uuid: 'uuid-nl'
      }
    ]
    const incoming: SingBoxOutbound[] = [
      {
        type: 'vless',
        tag: '🇩🇪 VL | 德国keybe',
        server: '104.25.240.201',
        server_port: 443,
        uuid: 'uuid-de'
      },
      {
        type: 'vless',
        tag: '🇳🇱 TR | 荷兰',
        server: '104.25.240.201',
        server_port: 443,
        uuid: 'uuid-nl'
      }
    ]

    const merged = mergePreservedNodeSettings(existing, incoming)
    assert.equal(merged[0].detour, 'profile-a::relay')
    assert.equal(merged[1].detour, undefined)
  })

  it('preserves manually added nodes (x_kunbox_manual) when subscription updates', () => {
    const existing: SingBoxOutbound[] = [
      { type: 'vless', tag: 'remote-1', server: '1.1.1.1', server_port: 443, uuid: 'u1' },
      {
        type: 'vless',
        tag: 'ru2-direct',
        server: '46.149.227.209',
        server_port: 18443,
        uuid: 'manual-uuid',
        detour: 'lxy::HK',
        x_kunbox_manual: true
      }
    ]
    const incoming: SingBoxOutbound[] = [
      { type: 'vless', tag: 'remote-1', server: '1.1.1.2', server_port: 443, uuid: 'u1' },
      { type: 'vless', tag: 'remote-2', server: '2.2.2.2', server_port: 443, uuid: 'u2' }
    ]

    const merged = mergePreservedNodeSettings(existing, incoming)
    assert.deepEqual(merged.map((n) => n.tag), ['remote-1', 'remote-2', 'ru2-direct'])
    assert.equal(merged[2].x_kunbox_manual, true)
    assert.equal(merged[2].detour, 'lxy::HK')
  })

  it('dedupeNodeTags avoids reserved outbound tags consistent with configGenerator', () => {
    const deduped = dedupeNodeTags([
      { type: 'socks', tag: 'PROXY', server: '1.1.1.1', server_port: 1080 },
      { type: 'socks', tag: 'direct', server: '1.1.1.2', server_port: 1080 },
      { type: 'socks', tag: 'auto', server: '1.1.1.3', server_port: 1080 },
      { type: 'socks', tag: 'block', server: '1.1.1.4', server_port: 1080 }
    ])
    assert.deepEqual(deduped.map((n) => n.tag), ['PROXY (2)', 'direct (2)', 'auto (2)', 'block (2)'])
  })
})
