import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { createServer } from 'node:http'
import {
  mkdtempSync,
  mkdirSync,
  copyFileSync,
  readFileSync,
  writeFileSync,
  rmSync,
  existsSync
} from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { fileURLToPath } from 'node:url'
import {
  KernelManager,
  RAYBOX_ASSET_NAME,
  RAYBOX_BACKUP_DIR,
  RAYBOX_INSTALL_DIR,
  RAYBOX_MANIFEST_FILENAME,
  RAYBOX_RECEIPT_FILENAME,
  compareSemverDesc,
  isOfficialHttpsUrl,
  isSafeAssetName,
  parseWindowsReleaseTag,
  validateReleaseAssets
} from './KernelManager'

const REPO_ELECTRON_ROOT = fileURLToPath(new URL('../../..', import.meta.url))
const BUNDLED_RAYBOX_DIR = join(REPO_ELECTRON_ROOT, 'resources', 'libs', 'raybox')
const BUNDLED_LOCK_PATH = join(REPO_ELECTRON_ROOT, 'resources', 'libs', 'raybox-windows.lock.json')

function sha256Hex(buf: Buffer | string): string {
  return createHash('sha256').update(buf).digest('hex')
}

function makeFakeRelease(version: string, opts?: { tagName?: string; draft?: boolean; prerelease?: boolean }) {
  const names = ['manifest.json', RAYBOX_ASSET_NAME, 'LICENSE', 'RayBox-Protocol-LICENSE', 'SOURCE.txt']
  return {
    tag_name: opts?.tagName ?? `windows-v${version}`,
    draft: opts?.draft ?? false,
    prerelease: opts?.prerelease ?? false,
    published_at: '2026-11-08T00:00:00Z',
    assets: names.map(name => ({
      name,
      size: 16,
      digest: `sha256:${'a'.repeat(64)}` as string | null
    }))
  }
}

async function startMockServer(
  handler: (url: string, res: import('node:http').ServerResponse) => void
): Promise<{ origin: string; close: () => Promise<void> }> {
  const server = createServer((req, res) => {
    handler(req.url || '/', res)
  })
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  const addr = server.address() as import('node:net').AddressInfo
  return {
    origin: `http://127.0.0.1:${addr.port}`,
    close: () => new Promise<void>((resolve, reject) => server.close(err => (err ? reject(err) : resolve())))
  }
}

describe('KernelManager', () => {
  it('filters platform tags, skips draft/prerelease and sorts semver numerically', async () => {
    assert.deepEqual(parseWindowsReleaseTag('windows-v0.0.10')?.triplet, [0, 0, 10])
    for (const invalid of ['windows-v00.1.0', 'windows-v1.2', 'windows-v1.2.3-beta', 'android-v1.0.0', 'windows-abcdef']) {
      assert.equal(parseWindowsReleaseTag(invalid), null)
    }
    assert.ok(compareSemverDesc('0.0.10', '0.0.2') < 0)

    const page1 = Array.from({ length: 100 }, () => makeFakeRelease('0.0.2'))
    page1[0] = makeFakeRelease('9.9.9', { tagName: 'android-v9.9.9' })
    page1[1] = makeFakeRelease('8.8.8', { draft: true })
    page1[2] = makeFakeRelease('7.7.7', { prerelease: true })
    const page2 = [makeFakeRelease('0.0.10'), makeFakeRelease('0.0.1')]

    const srv = await startMockServer((url, res) => {
      if (url === '/releases?per_page=100&page=1') {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify(page1))
        return
      }
      if (url === '/releases?per_page=100&page=2') {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify(page2))
        return
      }
      res.writeHead(404)
      res.end()
    })

    const tmp = mkdtempSync(join(tmpdir(), 'kb-km-list-'))
    try {
      const km = new KernelManager({
        dataKernelDir: join(tmp, 'data'),
        bundleKernelDir: join(tmp, 'bundle'),
        cacheDir: join(tmp, 'cache'),
        apiRoot: srv.origin,
        downloadBaseUrl: srv.origin
      })
      const list = await km.getRemoteReleases(false)
      assert.deepEqual(
        list.map(r => r.version),
        ['0.0.10', '0.0.2', '0.0.1']
      )
    } finally {
      await srv.close()
      rmSync(tmp, { recursive: true, force: true })
    }
  })

  it('rejects invalid asset names, missing digest, oversized assets and untrusted URLs', () => {
    for (const bad of ['../evil.exe', 'C:evil.exe', 'CON', 'com1.txt', 'a/b', 'a\\b', RAYBOX_MANIFEST_FILENAME]) {
      assert.equal(isSafeAssetName(bad), false)
    }

    const r1 = makeFakeRelease('0.0.2')
    r1.assets[0].digest = null
    assert.throws(() => validateReleaseAssets(r1), /digest/)

    const r2 = makeFakeRelease('0.0.2')
    r2.assets[0].size = 3 * 1024 * 1024
    assert.throws(() => validateReleaseAssets(r2), /大小/)

    const r3 = makeFakeRelease('0.0.2')
    r3.assets.push({ ...r3.assets[1] })
    assert.throws(() => validateReleaseAssets(r3), /重复/)

    for (const ok of ['https://github.com/roseforljh/RayBox-rx', 'https://release-assets.githubusercontent.com/x']) {
      assert.equal(isOfficialHttpsUrl(ok), true)
    }
    for (const bad of [
      'http://github.com/x',
      'https://github.com.evil.invalid/x',
      'https://user@github.com/x',
      'https://github.com:444/x'
    ]) {
      assert.equal(isOfficialHttpsUrl(bad), false)
    }
  })

  it('installs verified RayBox release with annotated tag, creates initial backup, detects receipt tampering and rolls back', async () => {
    const tmp = mkdtempSync(join(tmpdir(), 'kb-km-e2e-'))
    const dataDir = join(tmp, 'data')
    const bundleDir = join(tmp, 'bundle')
    const cacheDir = join(tmp, 'cache')
    const bundledRaybox = join(bundleDir, RAYBOX_INSTALL_DIR)
    mkdirSync(bundledRaybox, { recursive: true })

    for (const f of [RAYBOX_ASSET_NAME, RAYBOX_MANIFEST_FILENAME, 'LICENSE', 'RayBox-Protocol-LICENSE', 'SOURCE.txt']) {
      copyFileSync(join(BUNDLED_RAYBOX_DIR, f), join(bundledRaybox, f))
    }
    const lockTarget = join(bundleDir, 'raybox-windows.lock.json')
    copyFileSync(BUNDLED_LOCK_PATH, lockTarget)

    const manifestRaw = readFileSync(join(BUNDLED_RAYBOX_DIR, RAYBOX_MANIFEST_FILENAME))
    const manifestObj = JSON.parse(manifestRaw.toString('utf-8'))
    const assetBuffers = new Map<string, Buffer>([['manifest.json', manifestRaw]])
    for (const name of [RAYBOX_ASSET_NAME, 'LICENSE', 'RayBox-Protocol-LICENSE', 'SOURCE.txt']) {
      assetBuffers.set(name, readFileSync(join(BUNDLED_RAYBOX_DIR, name)))
    }

    const releasePayload = {
      tag_name: manifestObj.tag,
      draft: false,
      prerelease: false,
      published_at: '2026-11-08T00:00:00Z',
      assets: Array.from(assetBuffers.entries()).map(([name, buf]) => ({
        name,
        size: buf.length,
        digest: `sha256:${sha256Hex(buf)}`
      }))
    }
    const tagSha = 'd'.repeat(40)

    const srv = await startMockServer((url, res) => {
      if (url === `/releases/tags/${manifestObj.tag}`) {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify(releasePayload))
        return
      }
      if (url === `/git/ref/tags/${manifestObj.tag}`) {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ object: { type: 'tag', sha: tagSha } }))
        return
      }
      if (url === `/git/tags/${tagSha}`) {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ object: { type: 'commit', sha: manifestObj.commit } }))
        return
      }
      const prefix = `/releases/download/${manifestObj.tag}/`
      if (url.startsWith(prefix)) {
        const assetName = url.slice(prefix.length)
        const buf = assetBuffers.get(assetName)
        if (buf) {
          res.writeHead(200, { 'Content-Type': 'application/octet-stream', 'Content-Length': String(buf.length) })
          res.end(buf)
          return
        }
      }
      res.writeHead(404)
      res.end()
    })

    let running = false
    let cleanupCalled = 0
    const km = new KernelManager({
      dataKernelDir: dataDir,
      bundleKernelDir: bundleDir,
      cacheDir,
      bundledLockPath: lockTarget,
      apiRoot: srv.origin,
      downloadBaseUrl: srv.origin,
      lifecycleHooks: {
        cleanupTempProcesses: () => {
          cleanupCalled += 1
        },
        isRunning: () => running,
        prepareForInstall: async () => ({ wasRunning: running }),
        restartAfterInstall: async () => ({ success: true }),
        stopAfterFailedRestart: async () => {
          running = false
        }
      }
    })

    try {
      const localBefore = await km.getLocalVersion()
      assert.equal(localBefore?.version, '0.0.1')
      const caps = await km.getCapabilities()
      assert.equal(caps.supportsNaive, false)
      assert.equal(caps.supportsIcmpProxy, true)
      assert.equal(await km.canRollback(), false)

      const phases: string[] = []
      km.on('downloadProgress', p => {
        if (p.phase && !phases.includes(p.phase)) phases.push(p.phase)
      })

      // Concurrent download guard
      const p1 = km.downloadKernel(manifestObj.tag)
      await assert.rejects(() => km.downloadKernel(manifestObj.tag), /另一个内核更新或回滚正在进行/)
      assert.equal(await p1, true)
      assert.deepEqual(phases, ['downloading', 'verifying', 'installing'])
      assert.ok(cleanupCalled >= 1)

      // Initial backup and source receipt created
      assert.equal(await km.canRollback(), true)
      assert.equal(existsSync(join(dataDir, RAYBOX_INSTALL_DIR, RAYBOX_RECEIPT_FILENAME)), true)
      assert.equal(existsSync(join(dataDir, RAYBOX_BACKUP_DIR, RAYBOX_ASSET_NAME)), true)

      const installed = await km.getInstalledVersions()
      assert.equal(installed.length, 2)
      assert.equal(installed.some(v => v.isBackup), true)

      // Running state blocks manual rollback
      running = true
      await assert.rejects(() => km.rollback(), /请先停止连接/)
      running = false

      // Rollback succeeds when stopped
      assert.equal(await km.rollback(), true)
      assert.equal((await km.getLocalVersion())?.version, '0.0.1')

      // Tampering source receipt causes runtime verification failure
      const receiptPath = join(dataDir, RAYBOX_INSTALL_DIR, RAYBOX_RECEIPT_FILENAME)
      writeFileSync(receiptPath, JSON.stringify({ repository: 'evil/repo', tag: 'x', commit: '0'.repeat(40), version: '9.9.9', manifestSha256: '0'.repeat(64), assets: {} }))
      await assert.rejects(() => km.resolveKernelPath(), /来源记录与 manifest 不一致/)
    } finally {
      await srv.close()
      rmSync(tmp, { recursive: true, force: true })
    }
  })

  it('rolls back automatically and stays stopped when restart after install fails', async () => {
    const tmp = mkdtempSync(join(tmpdir(), 'kb-km-fail-'))
    const dataDir = join(tmp, 'data')
    const bundleDir = join(tmp, 'bundle')
    const cacheDir = join(tmp, 'cache')
    const bundledRaybox = join(bundleDir, RAYBOX_INSTALL_DIR)
    mkdirSync(bundledRaybox, { recursive: true })
    for (const f of [RAYBOX_ASSET_NAME, RAYBOX_MANIFEST_FILENAME, 'LICENSE', 'RayBox-Protocol-LICENSE', 'SOURCE.txt']) {
      copyFileSync(join(BUNDLED_RAYBOX_DIR, f), join(bundledRaybox, f))
    }
    const lockTarget = join(bundleDir, 'raybox-windows.lock.json')
    copyFileSync(BUNDLED_LOCK_PATH, lockTarget)

    const manifestRaw = readFileSync(join(BUNDLED_RAYBOX_DIR, RAYBOX_MANIFEST_FILENAME))
    const manifestObj = JSON.parse(manifestRaw.toString('utf-8'))
    const assetBuffers = new Map<string, Buffer>([['manifest.json', manifestRaw]])
    for (const name of [RAYBOX_ASSET_NAME, 'LICENSE', 'RayBox-Protocol-LICENSE', 'SOURCE.txt']) {
      assetBuffers.set(name, readFileSync(join(BUNDLED_RAYBOX_DIR, name)))
    }

    const releasePayload = {
      tag_name: manifestObj.tag,
      draft: false,
      prerelease: false,
      assets: Array.from(assetBuffers.entries()).map(([name, buf]) => ({
        name,
        size: buf.length,
        digest: `sha256:${sha256Hex(buf)}`
      }))
    }

    const srv = await startMockServer((url, res) => {
      if (url === `/releases/tags/${manifestObj.tag}`) {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify(releasePayload))
        return
      }
      if (url === `/git/ref/tags/${manifestObj.tag}`) {
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ object: { type: 'commit', sha: manifestObj.commit } }))
        return
      }
      const prefix = `/releases/download/${manifestObj.tag}/`
      if (url.startsWith(prefix)) {
        const buf = assetBuffers.get(url.slice(prefix.length))
        if (buf) {
          res.writeHead(200, { 'Content-Type': 'application/octet-stream', 'Content-Length': String(buf.length) })
          res.end(buf)
          return
        }
      }
      res.writeHead(404)
      res.end()
    })

    let stopCalled = 0
    const km = new KernelManager({
      dataKernelDir: dataDir,
      bundleKernelDir: bundleDir,
      cacheDir,
      bundledLockPath: lockTarget,
      apiRoot: srv.origin,
      downloadBaseUrl: srv.origin,
      lifecycleHooks: {
        isRunning: () => false,
        prepareForInstall: async () => ({ wasRunning: true }),
        restartAfterInstall: async () => ({ success: false, error: '端口被占用' }),
        stopAfterFailedRestart: async () => {
          stopCalled += 1
        }
      }
    })

    try {
      await assert.rejects(() => km.downloadKernel(manifestObj.tag), /已恢复上一个 RayBox 工件，保持停止/)
      assert.equal(stopCalled, 1)
      // Verify restored installation works and does not carry the failed receipt
      assert.equal(existsSync(join(dataDir, RAYBOX_INSTALL_DIR, RAYBOX_RECEIPT_FILENAME)), false)
      assert.equal((await km.getLocalVersion())?.version, '0.0.1')
    } finally {
      await srv.close()
      rmSync(tmp, { recursive: true, force: true })
    }
  })
})
