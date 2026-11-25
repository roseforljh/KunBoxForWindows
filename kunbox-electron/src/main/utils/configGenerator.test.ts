import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { DEFAULT_SETTINGS, type DomainRule, type SingBoxOutbound } from '../../shared/types'
import { SingBoxManager } from '../core/SingBoxManager'
import { TrafficMonitor } from '../core/TrafficMonitor'
import { parseShareLink } from '../ipc/shareLink'
import { generateConfig, generateLatencyTestConfig, isProxyType } from './configGenerator'
import { getDataDir } from './paths'
import { resolveApiPort } from './proxyPorts'
import {
  DYNAMIC_RULE_SET_TAGS,
  domainRulesNeedRestart,
  getDynamicRuleSetPath,
  ruleSetsNeedRestart,
  syncBypassLanRuleSet,
  syncCustomDomainRuleSets,
  type RuleSetItem
} from './ruleSetManager'

const RAYBOX_EXE = join(process.cwd(), 'resources', 'libs', 'raybox', 'raybox-windows-amd64.exe')

function checkWithRealKernel(config: object): void {
  assert.equal(existsSync(RAYBOX_EXE), true, `找不到内置 RayBox 内核: ${RAYBOX_EXE}`)
  const dir = mkdtempSync(join(tmpdir(), 'kunbox-e2e-check-'))
  const configPath = join(dir, 'config.json')
  try {
    writeFileSync(configPath, JSON.stringify(config, null, 2), 'utf-8')
    const res = spawnSync(RAYBOX_EXE, ['check', '-c', configPath], {
      cwd: dir,
      encoding: 'utf-8',
      timeout: 10_000
    })
    assert.equal(
      res.status,
      0,
      `RayBox 内核配置校验失败 (exit=${res.status}):\n${res.stderr || res.stdout}`
    )
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

test('E2E: generateConfig handles FakeIP, detour chains, duplicate tags, and domain node/profile routing against real RayBox kernel', () => {
  const configsDir = join(getDataDir(), 'configs')
  mkdirSync(configsDir, { recursive: true })
  const tempProfileId = `e2e-cross-profile-${Date.now()}`
  const tempProfilePath = join(configsDir, `${tempProfileId}.json`)

  const crossNodes: SingBoxOutbound[] = [
    {
      type: 'socks',
      tag: 'FrontHop',
      server: '127.0.0.1',
      server_port: 10801
    },
    {
      type: 'http',
      tag: 'ProfileTarget',
      server: '127.0.0.1',
      server_port: 10802
    }
  ]

  writeFileSync(tempProfilePath, JSON.stringify(crossNodes, null, 2), 'utf-8')

  try {
    const primaryNodes: SingBoxOutbound[] = [
      {
        type: 'socks',
        tag: 'DupNode',
        server: '127.0.0.1',
        server_port: 10810,
        detour: `${tempProfileId}::FrontHop`
      },
      {
        type: 'socks',
        tag: 'DupNode',
        server: '127.0.0.1',
        server_port: 10811,
        detour: 'non-existent-profile::MissingNode'
      }
    ]

    const domainRules: DomainRule[] = [
      { id: 'r1', name: 'BlockAds', type: 'domain_suffix', value: 'ads.example.com', outboundMode: 'block', enabled: true },
      { id: 'r2', name: 'DirectSite', type: 'domain', value: 'direct.example.com', outboundMode: 'direct', enabled: true },
      { id: 'r3', name: 'NodeSite', type: 'domain_suffix', value: 'node.example.com', outboundMode: 'node', outboundValue: `${tempProfileId}::FrontHop`, enabled: true },
      { id: 'r4', name: 'ProfileSite', type: 'domain_keyword', value: 'profile-test', outboundMode: 'profile', outboundValue: tempProfileId, enabled: true }
    ]

    const config = generateConfig({
      settings: {
        ...DEFAULT_SETTINGS,
        fakeDns: true,
        tunEnabled: false,
        systemProxy: false
      },
      outbounds: primaryNodes,
      activeNodeTag: 'DupNode',
      domainRules,
      clashApiPort: 19091
    }) as any

    // 1. 验证 FakeIP 采用 sing-box 1.14 新格式，无旧顶层 dns.fakeip
    assert.equal(config.dns.fakeip, undefined)
    const fakeipServer = config.dns.servers.find((s: any) => s.type === 'fakeip')
    assert.equal(fakeipServer?.inet4_range, '198.18.0.0/15')

    // 2. 验证 dns.rules 不包含 geoip 规则集
    for (const rule of config.dns.rules) {
      if (Array.isArray(rule.rule_set)) {
        for (const tag of rule.rule_set) {
          assert.equal(String(tag).toLowerCase().includes('geoip'), false)
        }
      }
    }

    // 3. 验证重名节点去重、跨订阅前置代理注入、无效前置代理自动清理
    const tags = config.outbounds.map((o: any) => o.tag)
    assert.ok(tags.includes('DupNode'))
    assert.ok(tags.includes('DupNode (2)'))
    assert.ok(tags.includes('FrontHop'))
    assert.ok(tags.includes(`profile::${tempProfileId}`))
    assert.equal(tags.includes('block'), false)

    const dup1 = config.outbounds.find((o: any) => o.tag === 'DupNode')
    const dup2 = config.outbounds.find((o: any) => o.tag === 'DupNode (2)')
    assert.equal(dup1.detour, 'FrontHop')
    assert.equal(dup2.detour, undefined)

    const proxySelector = config.outbounds.find((o: any) => o.tag === 'PROXY')
    assert.equal(proxySelector?.default, 'DupNode')
    assert.equal(proxySelector?.interrupt_exist_connections, true)

    // 4. 验证自定义域名分流：block / direct 写入本地热载规则集，node -> FrontHop，profile -> profile::<id>
    const blockRule = config.route.rules.find((r: any) => r.rule_set?.includes(DYNAMIC_RULE_SET_TAGS.customBlock))
    assert.equal(blockRule?.action, 'reject')
    const blockSetJson = JSON.parse(readFileSync(getDynamicRuleSetPath(DYNAMIC_RULE_SET_TAGS.customBlock), 'utf-8'))
    assert.ok(blockSetJson.rules[0]?.domain_suffix?.includes('ads.example.com'))
    const directSetJson = JSON.parse(readFileSync(getDynamicRuleSetPath(DYNAMIC_RULE_SET_TAGS.customDirect), 'utf-8'))
    assert.ok(directSetJson.rules[0]?.domain?.includes('direct.example.com'))
    const nodeRule = config.route.rules.find((r: any) => r.domain_suffix?.includes('node.example.com'))
    assert.equal(nodeRule?.outbound, 'FrontHop')
    const profileRule = config.route.rules.find((r: any) => r.domain_keyword?.includes('profile-test'))
    assert.equal(profileRule?.outbound, `profile::${tempProfileId}`)

    // 5. 用真实 RayBox 内核执行 check 校验整份配置
    checkWithRealKernel(config)

    // 6. 用真实 RayBox 内核校验精准测速配置（含单端口 http inbound 与 auth_user 路由）
    const latencyConfig = generateLatencyTestConfig(primaryNodes, 19092, 19093)
    checkWithRealKernel(latencyConfig)
  } finally {
    rmSync(tempProfilePath, { force: true })
  }
})

test('E2E: real RayBox kernel boots on dynamic Clash API port, serves TrafficMonitor, and switches mode', async () => {
  assert.equal(existsSync(RAYBOX_EXE), true)

  // 先占用一个端口验证 resolveApiPort 会自动避让
  const blocker = createServer()
  await new Promise<void>((resolve) => blocker.listen(0, '127.0.0.1', resolve))
  const blockedPort = (blocker.address() as any).port
  const apiPort = await resolveApiPort(blockedPort)
  assert.notEqual(apiPort, blockedPort)
  blocker.close()

  const dir = mkdtempSync(join(tmpdir(), 'kunbox-e2e-live-'))
  const configPath = join(dir, 'latency.json')
  const config = generateLatencyTestConfig(
    [{ type: 'socks', tag: 'LoopbackNode', server: '127.0.0.1', server_port: 10899 }],
    apiPort
  )
  writeFileSync(configPath, JSON.stringify(config, null, 2), 'utf-8')

  const proc = spawn(RAYBOX_EXE, ['run', '-c', configPath], {
    cwd: dir,
    windowsHide: true,
    stdio: ['ignore', 'ignore', 'pipe']
  })

  const monitor = new TrafficMonitor(`http://127.0.0.1:${apiPort}`, () => {})

  try {
    // 等待内核 Clash API 就绪
    let ready = false
    const deadline = Date.now() + 5000
    while (Date.now() < deadline && !ready) {
      ready = await new Promise<boolean>((resolve) => {
        const req = http.get({ host: '127.0.0.1', port: apiPort, path: '/version', timeout: 500 }, (res) => {
          res.resume()
          resolve(res.statusCode === 200)
        })
        req.on('error', () => resolve(false))
        req.on('timeout', () => { req.destroy(); resolve(false) })
      })
      if (!ready) await new Promise((r) => setTimeout(r, 100))
    }
    assert.equal(ready, true, 'RayBox Clash API 未能在 5 秒内就绪')

    // 1. 验证 PATCH /configs 实时切换代理模式
    const patchStatus = await new Promise<number>((resolve, reject) => {
      const payload = JSON.stringify({ mode: 'global' })
      const req = http.request({
        host: '127.0.0.1',
        port: apiPort,
        path: '/configs',
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(payload) },
        timeout: 2000
      }, (res) => {
        res.resume()
        resolve(res.statusCode ?? 0)
      })
      req.on('error', reject)
      req.write(payload)
      req.end()
    })
    assert.equal(patchStatus, 204)

    // 2. 验证 TrafficMonitor 能收到真实 /connections 流量快照（无系统代理也能工作）
    const gotSnapshot = await new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => resolve(false), 3000)
      monitor.once('traffic', () => {
        clearTimeout(timer)
        resolve(true)
      })
      monitor.start()
    })
    assert.equal(gotSnapshot, true, 'TrafficMonitor 未收到真实内核流量事件')
  } finally {
    monitor.stop()
    if (proc.exitCode === null) {
      const exited = new Promise<void>((resolve) => proc.once('exit', () => resolve()))
      proc.kill('SIGKILL')
      await Promise.race([exited, new Promise((r) => setTimeout(r, 2000))])
    }
    rmSync(dir, { recursive: true, force: true })
  }
})

test('E2E: VLESS XHTTP + REALITY + Encryption converts to native xray outbound and passes real RayBox check', () => {
  const link =
    'vless://8b062f90-a96e-424c-9fb5-af08f0357947@35.212.174.125:34079?encryption=mlkem768x25519plus.native.0rtt.Z9xJHXwCD22Z_0xiwNhzOEZrRX6GRYDBP6d7CHPGoRU&flow=xtls-rprx-vision&security=reality&type=xhttp&sni=swdist.apple.com&fp=firefox&pbk=UlNIVQDev9mT_kDQxDQKah-scP4by-V6MNaxk-b0UXI&sid=d4501f3e217b6a7e&path=%2Fws-d92a98d2a03a631b&host=swdist.apple.com&mode=stream-one#node-dbeea602'
  const parsed = parseShareLink(link)
  assert.ok(parsed)
  assert.equal(isProxyType('xray'), true)

  // 同时覆盖历史导入把 XHTTP host 写入 headers.Host 的旧节点
  const legacyHeaderHostNode: SingBoxOutbound = {
    ...parsed,
    tag: 'legacy-xhttp-host',
    transport: {
      type: 'xhttp',
      path: '/ws-d92a98d2a03a631b',
      headers: { Host: 'swdist.apple.com' },
      mode: 'stream-one'
    }
  }

  const fullConfig = generateConfig({
    settings: { ...DEFAULT_SETTINGS, tunEnabled: false, systemProxy: false },
    outbounds: [parsed, legacyHeaderHostNode],
    activeNodeTag: parsed.tag,
    domainRules: [],
    clashApiPort: 19094
  }) as any

  const xrayOutbound = fullConfig.outbounds.find((o: any) => o.tag === 'node-dbeea602')
  assert.equal(xrayOutbound?.type, 'xray')
  assert.equal(xrayOutbound?.outbound?.protocol, 'vless')
  assert.equal(
    xrayOutbound?.outbound?.settings?.vnext?.[0]?.users?.[0]?.encryption,
    'mlkem768x25519plus.native.0rtt.Z9xJHXwCD22Z_0xiwNhzOEZrRX6GRYDBP6d7CHPGoRU'
  )
  assert.equal(xrayOutbound?.outbound?.settings?.vnext?.[0]?.users?.[0]?.flow, 'xtls-rprx-vision')
  assert.equal(xrayOutbound?.outbound?.streamSettings?.network, 'xhttp')
  assert.equal(xrayOutbound?.outbound?.streamSettings?.security, 'reality')
  assert.equal(xrayOutbound?.outbound?.streamSettings?.xhttpSettings?.host, 'swdist.apple.com')
  assert.equal(xrayOutbound?.outbound?.streamSettings?.xhttpSettings?.mode, 'stream-one')

  const legacyOutbound = fullConfig.outbounds.find((o: any) => o.tag === 'legacy-xhttp-host')
  assert.equal(legacyOutbound?.type, 'xray')
  assert.equal(legacyOutbound?.outbound?.streamSettings?.xhttpSettings?.host, 'swdist.apple.com')

  // 对齐 RayBox streamconn/option.XHTTPOptions：覆盖 httpupgrade + encryption、xhttp headers/packetDown/xmux、ws ed
  const rayboxExtendedNodes: SingBoxOutbound[] = [
    {
      tag: 'xhttp-tuned',
      type: 'vless',
      server: '127.0.0.1',
      server_port: 443,
      uuid: '11111111-1111-4111-8111-111111111111',
      encryption: 'mlkem768x25519plus.native.0rtt.Z9xJHXwCD22Z_0xiwNhzOEZrRX6GRYDBP6d7CHPGoRU',
      tls: { enabled: true, server_name: 'edge.example.invalid', utls: { enabled: true, fingerprint: 'chrome_155' } },
      transport: {
        type: 'xhttp',
        host: 'edge.example.invalid',
        path: '/tuned',
        mode: 'packet-up',
        headers: { Host: 'ignored-by-host-field', 'X-Trace': 'ok' },
        extra: {
          packetDown: { keepAlive: true, keepAlivePeriod: '10-20', keepAliveLength: '24-64' },
          xmux: { maxConcurrency: 5, maxConnections: 0 }
        }
      }
    },
    {
      tag: 'httpupgrade-enc',
      type: 'vless',
      server: '127.0.0.1',
      server_port: 443,
      uuid: '11111111-1111-4111-8111-111111111111',
      encryption: 'mlkem768x25519plus.native.0rtt.Z9xJHXwCD22Z_0xiwNhzOEZrRX6GRYDBP6d7CHPGoRU',
      tls: { enabled: true, server_name: 'edge.example.invalid' },
      transport: { type: 'httpupgrade', host: 'edge.example.invalid', path: '/up', headers: { 'X-Up': '1' } }
    },
    {
      tag: 'ws-ed-enc',
      type: 'vless',
      server: '127.0.0.1',
      server_port: 443,
      uuid: '11111111-1111-4111-8111-111111111111',
      encryption: 'mlkem768x25519plus.native.0rtt.Z9xJHXwCD22Z_0xiwNhzOEZrRX6GRYDBP6d7CHPGoRU',
      tls: { enabled: true, server_name: 'edge.example.invalid' },
      transport: { type: 'ws', path: '/ws', max_early_data: 2048, early_data_header_name: 'Sec-WebSocket-Protocol', headers: { Host: 'edge.example.invalid' } }
    }
  ]

  const extendedConfig = generateLatencyTestConfig([parsed, legacyHeaderHostNode, ...rayboxExtendedNodes], 19095, 19096) as any
  const tunedOut = extendedConfig.outbounds.find((o: any) => o.tag === 'xhttp-tuned')
  assert.deepEqual(tunedOut?.outbound?.streamSettings?.xhttpSettings?.headers, { 'X-Trace': 'ok' })
  assert.equal(tunedOut?.outbound?.streamSettings?.xhttpSettings?.extra?.packetDown?.keepAlive, true)
  const huOut = extendedConfig.outbounds.find((o: any) => o.tag === 'httpupgrade-enc')
  assert.equal(huOut?.outbound?.streamSettings?.network, 'httpupgrade')
  assert.equal(huOut?.outbound?.streamSettings?.httpupgradeSettings?.host, 'edge.example.invalid')
  const wsOut = extendedConfig.outbounds.find((o: any) => o.tag === 'ws-ed-enc')
  assert.equal(wsOut?.outbound?.streamSettings?.wsSettings?.path, '/ws?ed=2048')

  checkWithRealKernel(fullConfig)
  checkWithRealKernel(extendedConfig)
})

test('E2E: real RayBox kernel hot-switches clash modes, flushes DNS cache, and reloads local dynamic rule-sets via fswatch', async () => {
  assert.equal(existsSync(RAYBOX_EXE), true)

  const apiPort = await resolveApiPort(19120)
  const localPort = await resolveApiPort(apiPort + 1)
  const socksPort = await resolveApiPort(localPort + 1)

  const dir = mkdtempSync(join(tmpdir(), 'kunbox-e2e-hotreload-'))
  const configPath = join(dir, 'config.json')
  const config = generateConfig({
    settings: {
      ...DEFAULT_SETTINGS,
      localPort,
      socksPort,
      tunEnabled: false,
      systemProxy: false,
      routingMode: 'rule',
      bypassLan: true
    },
    outbounds: [{ type: 'socks', tag: 'HotNode', server: '127.0.0.1', server_port: 10899 }],
    activeNodeTag: 'HotNode',
    domainRules: [],
    clashApiPort: apiPort
  })
  writeFileSync(configPath, JSON.stringify(config, null, 2), 'utf-8')

  let stderrLog = ''
  const proc = spawn(RAYBOX_EXE, ['run', '-c', configPath], {
    cwd: dir,
    windowsHide: true,
    stdio: ['ignore', 'ignore', 'pipe']
  })
  proc.stderr?.on('data', (chunk: Buffer) => {
    stderrLog += chunk.toString('utf-8')
  })

  const apiRequest = (method: string, path: string, body?: object) =>
    new Promise<{ status: number; text: string }>((resolve, reject) => {
      const payload = body ? JSON.stringify(body) : undefined
      const req = http.request({
        host: '127.0.0.1',
        port: apiPort,
        path,
        method,
        headers: payload ? { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(payload) } : undefined,
        timeout: 3000
      }, (res) => {
        let text = ''
        res.on('data', (c) => { text += c })
        res.on('end', () => resolve({ status: res.statusCode ?? 0, text }))
      })
      req.on('error', reject)
      req.on('timeout', () => { req.destroy(); reject(new Error('timeout')) })
      if (payload) req.write(payload)
      req.end()
    })

  try {
    let ready = false
    const deadline = Date.now() + 5000
    while (Date.now() < deadline && !ready) {
      try {
        const res = await apiRequest('GET', '/version')
        ready = res.status === 200
      } catch {
        // retry
      }
      if (!ready) await new Promise((r) => setTimeout(r, 80))
    }
    assert.equal(ready, true, `RayBox 启动超时:\n${stderrLog}`)

    // 1. 验证 rule / global / direct 三态均在 modeList 中且 PATCH /configs 真正生效
    for (const targetMode of ['global', 'direct', 'rule'] as const) {
      const patchRes = await apiRequest('PATCH', '/configs', { mode: targetMode })
      assert.equal(patchRes.status, 204)
      const getRes = await apiRequest('GET', '/configs')
      const parsed = JSON.parse(getRes.text)
      assert.equal(String(parsed.mode).toLowerCase(), targetMode)
    }

    // 2. 验证 POST /cache/dns/flush 热清缓存返回 204
    const flushRes = await apiRequest('POST', '/cache/dns/flush')
    assert.equal(flushRes.status, 204)

    // 3. 验证运行期覆写本地动态规则集（bypassLan 与自定义域名）后 fswatch 热重载无报错
    syncBypassLanRuleSet(false)
    syncCustomDomainRuleSets([
      { id: 'h1', name: 'HotDirect', type: 'domain_suffix', value: 'hot-direct.test', outboundMode: 'direct', enabled: true },
      { id: 'h2', name: 'HotBlock', type: 'domain', value: 'hot-block.test', outboundMode: 'block', enabled: true }
    ])
    await new Promise((r) => setTimeout(r, 350))
    syncBypassLanRuleSet(true)
    syncCustomDomainRuleSets([])
    await new Promise((r) => setTimeout(r, 250))

    assert.equal(proc.exitCode, null, `热重载后内核异常退出:\n${stderrLog}`)
    assert.equal(/reload rule-set|FATAL|ERROR/i.test(stderrLog), false, `热重载产生内核报错:\n${stderrLog}`)
  } finally {
    if (proc.exitCode === null) {
      const exited = new Promise<void>((resolve) => proc.once('exit', () => resolve()))
      proc.kill('SIGKILL')
      await Promise.race([exited, new Promise((r) => setTimeout(r, 2000))])
    }
    rmSync(dir, { recursive: true, force: true })
  }
})

test('needsRestart判定：仅实质影响内核结构的规则/规则集变更才要求重启', () => {
  const baseDomain: DomainRule[] = [
    { id: '1', name: 'a', type: 'domain_suffix', value: 'a.com', outboundMode: 'direct', enabled: true },
    { id: '2', name: 'b', type: 'domain', value: 'b.com', outboundMode: 'node', outboundValue: 'p1::n1', enabled: false }
  ]
  // 增删改 direct/proxy/block 或禁用态 node 规则均无需重启
  assert.equal(domainRulesNeedRestart(baseDomain, [
    ...baseDomain,
    { id: '3', name: 'c', type: 'domain_keyword', value: 'ads', outboundMode: 'block', enabled: true }
  ]), false)
  // 启用 node 规则才需要重启
  assert.equal(domainRulesNeedRestart(baseDomain, [
    baseDomain[0],
    { ...baseDomain[1], enabled: true }
  ]), true)

  const baseSets: RuleSetItem[] = [
    { id: '1', tag: 'rs-local', name: '本地', url: 'a.json', path: 'a.json', type: 'local', format: 'source', outboundMode: 'direct', enabled: true, isBuiltIn: false },
    { id: '2', tag: 'rs-off', name: '禁用项', url: 'b.json', path: 'b.json', type: 'local', format: 'source', outboundMode: 'proxy', enabled: false, isBuiltIn: false }
  ]
  // 改显示名称、删除禁用规则集无需重启
  assert.equal(ruleSetsNeedRestart(baseSets, [
    { ...baseSets[0], name: '改个名' }
  ]), false)
  // 启用新规则集需要重启
  assert.equal(ruleSetsNeedRestart(baseSets, [
    baseSets[0],
    { ...baseSets[1], enabled: true }
  ]), true)
})

test('E2E: optimized TUN config passes real RayBox check and SingBoxManager restart completes rapidly', async () => {
  assert.equal(existsSync(RAYBOX_EXE), true)

  // 1. 校验优化后的 TUN 配置（自动最优 MTU、纯 IPv4、50ms 精准嗅探、DNS 乐观缓存、warn 日志级别）通过真实 RayBox check
  const tunConfig = generateConfig({
    settings: {
      ...DEFAULT_SETTINGS,
      tunEnabled: true,
      tunStack: 'mixed',
      tunStrictRoute: false,
      fakeDns: true,
      systemProxy: false
    },
    outbounds: [{ type: 'socks', tag: 'TunNode', server: '127.0.0.1', server_port: 10899 }],
    activeNodeTag: 'TunNode',
    domainRules: [],
    clashApiPort: 19130
  }) as any

  const tunInbound = tunConfig.inbounds.find((i: any) => i.type === 'tun')
  assert.equal(tunInbound?.mtu, undefined)
  assert.deepEqual(tunInbound?.address, ['172.19.0.1/30'])
  assert.equal(tunInbound?.stack, 'mixed')
  assert.equal(tunInbound?.udp_timeout, '5m')
  assert.equal(tunConfig.dns.cache_capacity, 4096)
  assert.equal(tunConfig.dns.optimistic?.enabled, true)
  assert.equal(tunConfig.log.level, 'warn')
  assert.equal(tunConfig.route.rules[0]?.action, 'hijack-dns')
  assert.deepEqual(tunConfig.route.rules[0]?.port, [53])
  const sniffRule = tunConfig.route.rules.find((r: any) => r.action === 'sniff')
  assert.equal(sniffRule?.timeout, '50ms')
  checkWithRealKernel(tunConfig)

  // 2. 实测 SingBoxManager 真实进程重启端到端耗时与可用性
  const apiPort = await resolveApiPort(19131)
  const localPort = await resolveApiPort(apiPort + 1)
  const socksPort = await resolveApiPort(localPort + 1)
  const dir = mkdtempSync(join(tmpdir(), 'kunbox-e2e-restart-'))
  const mgr = new SingBoxManager({
    execPath: RAYBOX_EXE,
    configDir: dir,
    workDir: dir,
    apiHost: '127.0.0.1',
    apiPort,
    apiSecret: ''
  }, () => {})

  const content = JSON.stringify(generateConfig({
    settings: {
      ...DEFAULT_SETTINGS,
      localPort,
      socksPort,
      tunEnabled: false,
      systemProxy: false
    },
    outbounds: [{ type: 'socks', tag: 'RestartNode', server: '127.0.0.1', server_port: 10899 }],
    activeNodeTag: 'RestartNode',
    domainRules: [],
    clashApiPort: apiPort
  }), null, 2)

  try {
    const startRes = await mgr.start({ configContent: content })
    assert.equal(startRes.success, true, startRes.error || 'start failed')

    const t0 = Date.now()
    const restartRes = await mgr.restart({ configContent: content })
    const elapsed = Date.now() - t0
    assert.equal(restartRes.success, true, restartRes.error || 'restart failed')
    assert.equal(mgr.isRunning(), true)
    assert.ok(elapsed < 2000, `重启耗时过长: ${elapsed}ms`)
  } finally {
    await mgr.stop({ force: true, timeout: 1000 })
    mgr.destroy()
    rmSync(dir, { recursive: true, force: true })
  }
})

test('E2E: startup onStarted sync overrides stale cache.db PROXY selection against real RayBox kernel', async () => {
  assert.equal(existsSync(RAYBOX_EXE), true)

  const apiPort = await resolveApiPort(19140)
  const localPort = await resolveApiPort(apiPort + 1)
  const socksPort = await resolveApiPort(localPort + 1)
  const dir = mkdtempSync(join(tmpdir(), 'kunbox-e2e-cachedb-node-'))
  const mgr = new SingBoxManager({
    execPath: RAYBOX_EXE,
    configDir: dir,
    workDir: dir,
    apiHost: '127.0.0.1',
    apiPort,
    apiSecret: ''
  }, () => {})

  const nodes: SingBoxOutbound[] = [
    { type: 'socks', tag: 'Node1', server: '127.0.0.1', server_port: 10891 },
    { type: 'socks', tag: 'Node2', server: '127.0.0.1', server_port: 10892 }
  ]

  const buildContent = (activeNodeTag: string) => JSON.stringify(generateConfig({
    settings: {
      ...DEFAULT_SETTINGS,
      localPort,
      socksPort,
      tunEnabled: false,
      systemProxy: false
    },
    outbounds: nodes,
    activeNodeTag,
    domainRules: [],
    clashApiPort: apiPort
  }), null, 2)

  try {
    // 1. 首次启动选中 Node1，随后热切到 Node2，使 RayBox 将 PROXY=Node2 持久化写入 cache.db
    const firstStart = await mgr.start({ configContent: buildContent('Node1') })
    assert.equal(firstStart.success, true, firstStart.error || 'first start failed')
    assert.equal(await mgr.getProxyNow('PROXY'), 'Node1')

    const switched = await mgr.selectProxy('PROXY', 'Node2')
    assert.equal(switched.success, true, switched.error || 'selectProxy Node2 failed')
    assert.equal(await mgr.getProxyNow('PROXY'), 'Node2')
    await mgr.stop({ force: true, timeout: 1500 })

    // 2. 离线改为选中 Node1 后重启：确认 cache.db 确实会把启动瞬间的 PROXY 覆写为 Node2，
    //    而 onStarted 在进入 running 前将其纠正为 Node1 并写回 cache.db
    let beforeSyncNow: string | null = null
    const secondStart = await mgr.start({
      prepareConfig: async () => ({
        content: buildContent('Node1'),
        onStarted: async () => {
          beforeSyncNow = await mgr.getProxyNow('PROXY')
          if (beforeSyncNow !== 'Node1') {
            const syncRes = await mgr.selectProxy('PROXY', 'Node1')
            assert.equal(syncRes.success, true, syncRes.error || 'sync Node1 failed')
          }
        }
      })
    })
    assert.equal(secondStart.success, true, secondStart.error || 'second start failed')
    assert.equal(beforeSyncNow, 'Node2', '应复现 cache.db 历史选中覆盖 default 的内核行为')
    assert.equal(await mgr.getProxyNow('PROXY'), 'Node1', '进入 running 后内核实际生效节点必须是 Node1')
  } finally {
    await mgr.stop({ force: true, timeout: 1000 })
    mgr.destroy()
    rmSync(dir, { recursive: true, force: true })
  }
})
