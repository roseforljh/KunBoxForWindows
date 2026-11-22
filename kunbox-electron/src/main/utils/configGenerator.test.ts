import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { DEFAULT_SETTINGS, type DomainRule, type SingBoxOutbound } from '../../shared/types'
import { TrafficMonitor } from '../core/TrafficMonitor'
import { generateConfig, generateLatencyTestConfig } from './configGenerator'
import { getDataDir } from './paths'
import { resolveApiPort } from './proxyPorts'

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

    // 4. 验证域名分流 block -> action: reject，node -> FrontHop，profile -> profile::<id>
    const blockRule = config.route.rules.find((r: any) => r.domain_suffix?.includes('ads.example.com'))
    assert.equal(blockRule?.action, 'reject')
    const nodeRule = config.route.rules.find((r: any) => r.domain_suffix?.includes('node.example.com'))
    assert.equal(nodeRule?.outbound, 'FrontHop')
    const profileRule = config.route.rules.find((r: any) => r.domain_keyword?.includes('profile-test'))
    assert.equal(profileRule?.outbound, `profile::${tempProfileId}`)

    // 5. 用真实 RayBox 内核执行 check 校验整份配置
    checkWithRealKernel(config)

    // 6. 用真实 RayBox 内核校验离线测速配置
    const latencyConfig = generateLatencyTestConfig(primaryNodes, 19092)
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
