import { execFile, spawn } from 'child_process'
import { promisify } from 'util'
import { createHash, randomUUID } from 'crypto'
import {
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  readdirSync,
  statSync,
  copyFileSync,
  renameSync,
  rmSync,
  openSync,
  readSync,
  closeSync,
  createWriteStream,
  createReadStream,
  unlinkSync
} from 'fs'
import { join, dirname } from 'path'
import { EventEmitter } from 'events'
import * as https from 'https'
import * as http from 'http'

const execFileAsync = promisify(execFile)

export const RAYBOX_REPO_SLUG = 'roseforljh/RayBox-rx'
export const RAYBOX_REPO_URL = `https://github.com/${RAYBOX_REPO_SLUG}`
export const RAYBOX_API_ROOT = `https://api.github.com/repos/${RAYBOX_REPO_SLUG}`
export const RAYBOX_ASSET_NAME = 'raybox-windows-amd64.exe'
export const RAYBOX_MANIFEST_FILENAME = 'raybox-windows-manifest.json'
export const RAYBOX_RECEIPT_FILENAME = '.raybox-source.json'
export const RAYBOX_INSTALL_DIR = 'raybox'
export const RAYBOX_BACKUP_DIR = 'raybox.backup'
const RAYBOX_CANDIDATE_ENV = 'KUNBOX_RAYBOX_WINDOWS_CANDIDATE'

const MAX_METADATA_BYTES = 2 * 1024 * 1024
const MAX_KERNEL_DOWNLOAD_SIZE_BYTES = 100 * 1024 * 1024
const MAX_RELEASE_BYTES = 256 * 1024 * 1024
const MAX_KERNEL_RELEASE_ASSETS = 16
const MAX_RELEASE_PAGES = 20
const MAX_REDIRECTS = 5

const REQUIRED_RELEASE_ASSETS = [
  'manifest.json',
  RAYBOX_ASSET_NAME,
  'LICENSE',
  'RayBox-Protocol-LICENSE',
  'SOURCE.txt'
] as const

const REQUIRED_LOCAL_ASSETS = [
  RAYBOX_ASSET_NAME,
  'LICENSE',
  'RayBox-Protocol-LICENSE',
  'SOURCE.txt'
] as const

const ALLOWED_GITHUB_HOSTS = new Set([
  'github.com',
  'api.github.com',
  'release-assets.githubusercontent.com',
  'objects.githubusercontent.com',
  'github-releases.githubusercontent.com'
])

export interface KernelVersion {
  version: string
  versionDetail: string
  isAlpha: boolean
}

export interface RemoteRelease {
  version: string
  tagName: string
  publishedAt: string
  isPrerelease: boolean
  downloadUrl: string
  assetName: string
}

export interface DownloadProgress {
  phase?: 'downloading' | 'verifying' | 'installing'
  downloaded: number
  total: number
  percent: number
}

export interface InstalledKernel {
  version: string
  versionDetail: string
  isBackup: boolean
  path: string
}

export interface KernelCapabilities {
  version: string
  supportsNaive: boolean
  supportsIcmpProxy: boolean
  supportsBypassAction: boolean
}

export interface KernelLifecycleHooks {
  prepareForInstall?: () => Promise<{ wasRunning: boolean }>
  restartAfterInstall?: () => Promise<{ success: boolean; error?: string }>
  stopAfterFailedRestart?: () => Promise<void>
  isRunning?: () => boolean
  cleanupTempProcesses?: () => Promise<void> | void
}

export interface KernelManagerOptions {
  dataKernelDir: string
  bundleKernelDir: string
  cacheDir: string
  bundledLockPath?: string
  isDev?: boolean
  apiRoot?: string
  downloadBaseUrl?: string
  logger?: (msg: string) => void
  lifecycleHooks?: KernelLifecycleHooks
}

interface GithubReleaseAsset {
  name: string
  size: number
  digest?: string | null
  browser_download_url?: string
}

interface GithubRelease {
  tag_name: string
  draft: boolean
  prerelease: boolean
  published_at?: string | null
  assets: GithubReleaseAsset[]
}

interface RayBoxAssetEntry {
  file: string
  size: number
  sha256: string
}

interface RayBoxManifest {
  schemaVersion: number
  repository: string
  commit: string
  fullHead: string
  tag: string | null
  dirty: boolean
  sourceDigest: string
  sourceDigestAlgorithm: string
  source: {
    url: string
    runtimeLicense: string
    protocolLicense: string
  }
  product: string
  version: string
  go: string
  goVersion: string
  goos: string
  goarch: string
  buildTags: string[]
  rxDefault: boolean
  oobDefault: boolean
  pe: {
    machineCode: string
  }
  assets: Record<string, RayBoxAssetEntry>
}

interface RayBoxSourceReceipt {
  repository: string
  tag: string
  commit: string
  version: string
  manifestSha256: string
  assets: Record<string, string>
}

interface RayBoxLock {
  repository: string
  commit: string
  tag: string
  manifestSha256: string
  sourceDigest: string
  product: string
  version: string
}

interface RayBoxDownloadTarget {
  tag: string
  commit: string
  version: string
  manifestSha256: string
  assets: Map<string, GithubReleaseAsset>
}

export function isLowerHex(value: unknown, length: number): value is string {
  return typeof value === 'string' && value.length === length && /^[0-9a-f]+$/.test(value)
}

export function strictSemverTriplet(version: string): [number, number, number] | null {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(version)
  if (!match) return null
  return [Number(match[1]), Number(match[2]), Number(match[3])]
}

export function compareSemverDesc(a: string, b: string): number {
  const ta = strictSemverTriplet(a)
  const tb = strictSemverTriplet(b)
  if (!ta && !tb) return 0
  if (!ta) return 1
  if (!tb) return -1
  for (let i = 0; i < 3; i += 1) {
    if (tb[i] !== ta[i]) return tb[i] - ta[i]
  }
  return 0
}

export function parseWindowsReleaseTag(
  tag: string
): { version: string; triplet: [number, number, number] } | null {
  if (!tag.startsWith('windows-v')) return null
  const version = tag.slice('windows-v'.length)
  const triplet = strictSemverTriplet(version)
  if (!triplet) return null
  return { version, triplet }
}

export function isSafeAssetName(name: string): boolean {
  if (!name || name.length > 128 || name.endsWith('.')) return false
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name)) return false
  const stem = name.split('.')[0].toUpperCase()
  if (
    stem === 'CON' ||
    stem === 'PRN' ||
    stem === 'AUX' ||
    stem === 'NUL' ||
    /^COM[1-9]$/.test(stem) ||
    /^LPT[1-9]$/.test(stem)
  ) {
    return false
  }
  if (name.toLowerCase() === RAYBOX_MANIFEST_FILENAME) return false
  return true
}

export function isOfficialHttpsUrl(rawUrl: string, allowedTestOrigin?: string): boolean {
  try {
    const parsed = new URL(rawUrl)
    if (allowedTestOrigin && parsed.origin === allowedTestOrigin) {
      return !parsed.username && !parsed.password
    }
    if (parsed.protocol !== 'https:') return false
    if (parsed.username || parsed.password) return false
    if (parsed.port && parsed.port !== '443') return false
    return ALLOWED_GITHUB_HOSTS.has(parsed.hostname.toLowerCase())
  } catch {
    return false
  }
}

export function releaseAssetDigest(asset: GithubReleaseAsset): string {
  const raw = asset.digest
  if (typeof raw === 'string' && raw.startsWith('sha256:')) {
    const hex = raw.slice('sha256:'.length)
    if (isLowerHex(hex, 64)) return hex
  }
  throw new Error(`RayBox release 资产 ${asset.name} 缺少有效 SHA-256 digest`)
}

export function validateReleaseAssets(release: GithubRelease): Map<string, GithubReleaseAsset> {
  if (!Array.isArray(release.assets) || release.assets.length > MAX_KERNEL_RELEASE_ASSETS) {
    throw new Error('RayBox release 资产数量异常')
  }
  const assets = new Map<string, GithubReleaseAsset>()
  const namesLower = new Set<string>()
  let total = 0

  for (const asset of release.assets) {
    const lower = asset.name?.toLowerCase() || ''
    if (
      !isSafeAssetName(asset.name) ||
      namesLower.has(lower) ||
      typeof asset.size !== 'number' ||
      asset.size <= 0 ||
      asset.size > MAX_KERNEL_DOWNLOAD_SIZE_BYTES ||
      (asset.name === 'manifest.json' && asset.size > MAX_METADATA_BYTES)
    ) {
      throw new Error(`RayBox release 资产名称、大小或重复文件无效: ${asset.name}`)
    }
    namesLower.add(lower)
    releaseAssetDigest(asset)
    total += asset.size
    if (total > MAX_RELEASE_BYTES) {
      throw new Error('RayBox release 总大小超过限制')
    }
    assets.set(asset.name, asset)
  }

  for (const required of REQUIRED_RELEASE_ASSETS) {
    if (!assets.has(required)) {
      throw new Error(`RayBox release 缺少资产: ${required}`)
    }
  }

  return assets
}

function validRayboxIdentity(
  commit: string,
  version: string,
  tag: string | null | undefined,
  dirty: boolean
): boolean {
  if (!isLowerHex(commit, 40)) return false
  const legacy = version === `1.0.0-${commit.slice(0, 12)}`
  const semantic = strictSemverTriplet(version) !== null
  if (dirty) {
    return (tag === null || tag === undefined) && (legacy || semantic)
  }
  return (
    (legacy && tag === `windows-${commit}`) || (semantic && tag === `windows-v${version}`)
  )
}

function rayboxRepositoryMatches(value: string): boolean {
  return value === RAYBOX_REPO_URL || value === RAYBOX_REPO_SLUG
}

// 对大文件（如 80MB RayBox 内核）按 size + mtime + ctime 缓存 SHA-256，避免每次重启重复全盘读取
const LARGE_FILE_HASH_CACHE_THRESHOLD = 1024 * 1024
const largeFileSha256Cache = new Map<string, { key: string; hash: string }>()

function sha256File(filePath: string): Promise<string> {
  let cacheKey: string | null = null
  try {
    const st = statSync(filePath)
    if (st.size >= LARGE_FILE_HASH_CACHE_THRESHOLD) {
      cacheKey = `${st.size}:${st.mtimeMs}:${st.ctimeMs}`
      const cached = largeFileSha256Cache.get(filePath)
      if (cached && cached.key === cacheKey) {
        return Promise.resolve(cached.hash)
      }
    }
  } catch {
    // 交给后续流读取抛出原始错误
  }

  return new Promise((resolve, reject) => {
    const hash = createHash('sha256')
    const stream = createReadStream(filePath)
    stream.on('data', chunk => hash.update(chunk))
    stream.on('end', () => {
      const digest = hash.digest('hex')
      if (cacheKey) {
        if (largeFileSha256Cache.size >= 16) largeFileSha256Cache.clear()
        largeFileSha256Cache.set(filePath, { key: cacheKey, hash: digest })
      }
      resolve(digest)
    })
    stream.on('error', reject)
  })
}

function sha256Buffer(buf: Buffer | string): string {
  return createHash('sha256').update(buf).digest('hex')
}

function validatePeAmd64(executablePath: string): void {
  const stat = statSync(executablePath)
  if (stat.size < 0x40) {
    throw new Error('RayBox Windows 资产不是 PE 文件')
  }
  const fd = openSync(executablePath, 'r')
  try {
    const dosBuf = Buffer.alloc(0x40)
    readSync(fd, dosBuf, 0, 0x40, 0)
    if (dosBuf[0] !== 0x4d || dosBuf[1] !== 0x5a) {
      throw new Error('RayBox Windows 资产不是 PE 文件')
    }
    const peOffset = dosBuf.readUInt32LE(0x3c)
    if (peOffset + 6 > stat.size) {
      throw new Error('RayBox Windows PE 头截断')
    }
    const peHeader = Buffer.alloc(6)
    readSync(fd, peHeader, 0, 6, peOffset)
    if (
      peHeader[0] !== 0x50 ||
      peHeader[1] !== 0x45 ||
      peHeader[2] !== 0x00 ||
      peHeader[3] !== 0x00
    ) {
      throw new Error('RayBox Windows PE 签名无效')
    }
    const machine = peHeader.readUInt16LE(4)
    if (machine !== 0x8664) {
      throw new Error('RayBox Windows PE machine 不是 amd64')
    }
  } finally {
    closeSync(fd)
  }
}

export async function validateRayboxManifest(
  manifestPath: string,
  executablePath: string,
  allowDirty: boolean
): Promise<RayBoxManifest> {
  const text = readFileSync(manifestPath, 'utf-8')
  const manifest = JSON.parse(text) as RayBoxManifest
  const commit = manifest.commit

  if (
    manifest.schemaVersion !== 1 ||
    !rayboxRepositoryMatches(manifest.repository) ||
    !isLowerHex(commit, 40) ||
    manifest.fullHead !== commit ||
    (!allowDirty && manifest.dirty) ||
    !validRayboxIdentity(commit, manifest.version, manifest.tag, manifest.dirty)
  ) {
    throw new Error('RayBox Windows manifest 身份或 dirty 状态无效')
  }

  if (
    manifest.sourceDigestAlgorithm !== 'sha256-git-path-kind-content-v1' ||
    !isLowerHex(manifest.sourceDigest, 64) ||
    manifest.source?.url !== `https://github.com/${RAYBOX_REPO_SLUG}/tree/${commit}` ||
    manifest.source?.runtimeLicense !== 'docs/reference/runtime/LICENSE' ||
    manifest.source?.protocolLicense !== 'docs/reference/protocol/LICENSE'
  ) {
    throw new Error('RayBox Windows manifest 来源证明无效')
  }

  if (
    manifest.goos !== 'windows' ||
    manifest.goarch !== 'amd64' ||
    manifest.pe?.machineCode !== '0x8664' ||
    manifest.rxDefault ||
    manifest.oobDefault ||
    manifest.goVersion !== manifest.go ||
    !manifest.go?.startsWith('go version go') ||
    !manifest.go?.endsWith(' windows/amd64') ||
    !Array.isArray(manifest.buildTags) ||
    !manifest.buildTags.includes('with_clash_api') ||
    manifest.buildTags.includes('with_tailscale')
  ) {
    throw new Error('RayBox Windows manifest 平台、工具链或默认安全策略无效')
  }

  if (manifest.product !== 'RayBox') {
    throw new Error('RayBox Windows manifest 产品名称无效')
  }

  const assetsObj = manifest.assets
  if (!assetsObj || !assetsObj[RAYBOX_ASSET_NAME]) {
    throw new Error('RayBox Windows manifest 缺少 amd64 资产')
  }
  for (const required of REQUIRED_LOCAL_ASSETS) {
    if (!assetsObj[required]) {
      throw new Error(`RayBox Windows manifest 缺少资产: ${required}`)
    }
  }

  const root = dirname(manifestPath)
  for (const [name, entry] of Object.entries(assetsObj)) {
    if (
      name !== entry.file ||
      !isSafeAssetName(name) ||
      !isLowerHex(entry.sha256, 64)
    ) {
      throw new Error('RayBox Windows manifest 资产路径或 SHA-256 无效')
    }
    const targetPath = name === RAYBOX_ASSET_NAME ? executablePath : join(root, entry.file)
    if (!existsSync(targetPath)) {
      throw new Error(`RayBox Windows 资产文件不存在: ${entry.file}`)
    }
    const stat = statSync(targetPath)
    if (stat.size !== entry.size) {
      throw new Error(`RayBox Windows 资产大小不匹配: ${entry.file}`)
    }
    const actualHash = await sha256File(targetPath)
    if (actualHash !== entry.sha256) {
      throw new Error(`RayBox Windows 资产 SHA-256 不匹配: ${entry.file}`)
    }
  }

  validatePeAmd64(executablePath)

  const receiptPath = join(root, RAYBOX_RECEIPT_FILENAME)
  if (existsSync(receiptPath)) {
    const receipt = JSON.parse(readFileSync(receiptPath, 'utf-8')) as RayBoxSourceReceipt
    const manifestHash = sha256Buffer(Buffer.from(text, 'utf-8'))
    const receiptAssetKeys = Object.keys(receipt.assets || {})
    const manifestAssetKeys = Object.keys(assetsObj)
    const assetsMatch =
      receiptAssetKeys.length === manifestAssetKeys.length &&
      manifestAssetKeys.every(k => receipt.assets[k] === assetsObj[k].sha256)

    if (
      receipt.repository !== RAYBOX_REPO_SLUG ||
      receipt.commit !== manifest.commit ||
      receipt.tag !== (manifest.tag || '') ||
      receipt.version !== manifest.version ||
      receipt.manifestSha256 !== manifestHash ||
      !assetsMatch
    ) {
      throw new Error('RayBox 来源记录与 manifest 不一致')
    }
  }

  return manifest
}

function kernelSupportFileAvailable(kernelPath: string, filename: string): boolean {
  const localPath = join(dirname(kernelPath), filename)
  if (existsSync(localPath)) return true
  const envPath = process.env.PATH || ''
  for (const dir of envPath.split(';')) {
    if (dir && existsSync(join(dir, filename))) return true
  }
  return false
}

function buildCompatibilityConfig(): object {
  return {
    dns: {
      servers: [
        {
          tag: 'dns-bootstrap',
          type: 'udp',
          server: '223.5.5.5',
          server_port: 53
        }
      ]
    },
    inbounds: [
      {
        type: 'mixed',
        tag: 'local',
        listen: '127.0.0.1',
        listen_port: 12345
      }
    ],
    outbounds: [
      { type: 'direct', tag: 'direct' },
      {
        type: 'xray',
        tag: 'compatibility-xray',
        outbound: {
          protocol: 'vless',
          settings: {
            vnext: [
              {
                address: '127.0.0.1',
                port: 443,
                users: [
                  {
                    id: '00000000-0000-0000-0000-000000000001',
                    encryption: 'none'
                  }
                ]
              }
            ]
          },
          streamSettings: {
            network: 'xhttp',
            xhttpSettings: {
              path: '/check'
            }
          }
        }
      }
    ],
    route: {
      final: 'direct',
      auto_detect_interface: true
    },
    experimental: {
      clash_api: {
        external_controller: '127.0.0.1:12346'
      }
    }
  }
}

async function runWithTimeout(
  exe: string,
  args: string[],
  cwd: string,
  timeoutMs: number
): Promise<{ status: number; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(exe, args, {
      cwd,
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe']
    })
    let stdout = ''
    let stderr = ''
    let settled = false

    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      try {
        child.kill('SIGKILL')
      } catch {
        // ignore
      }
      reject(new Error('RayBox 内核兼容性检查超时，已终止检查进程'))
    }, timeoutMs)

    child.stdout?.on('data', chunk => {
      stdout += chunk.toString()
    })
    child.stderr?.on('data', chunk => {
      stderr += chunk.toString()
    })
    child.on('error', err => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      reject(err)
    })
    child.on('close', code => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve({ status: code ?? -1, stdout, stderr })
    })
  })
}

export class KernelManager extends EventEmitter {
  private dataKernelDir: string
  private bundleKernelDir: string
  private cacheDir: string
  private bundledLockPath: string
  private isDev: boolean
  private apiRoot: string
  private downloadBaseUrl: string
  private allowedTestOrigin?: string
  private logger: (msg: string) => void
  private lifecycleHooks: KernelLifecycleHooks
  private operationBusy = false

  constructor(options: KernelManagerOptions) {
    super()
    this.dataKernelDir = options.dataKernelDir
    this.bundleKernelDir = options.bundleKernelDir
    this.cacheDir = options.cacheDir
    this.bundledLockPath =
      options.bundledLockPath || join(this.bundleKernelDir, 'raybox-windows.lock.json')
    this.isDev = !!options.isDev
    this.apiRoot = (options.apiRoot || RAYBOX_API_ROOT).replace(/\/+$/, '')
    this.downloadBaseUrl = (options.downloadBaseUrl || RAYBOX_REPO_URL).replace(/\/+$/, '')
    if (options.apiRoot && options.apiRoot.startsWith('http://127.0.0.1:')) {
      this.allowedTestOrigin = new URL(options.apiRoot).origin
    } else if (options.downloadBaseUrl && options.downloadBaseUrl.startsWith('http://127.0.0.1:')) {
      this.allowedTestOrigin = new URL(options.downloadBaseUrl).origin
    }
    this.logger = options.logger || console.log
    this.lifecycleHooks = options.lifecycleHooks || {}

    mkdirSync(this.dataKernelDir, { recursive: true })
    mkdirSync(this.cacheDir, { recursive: true })
  }

  private getCandidateDir(): string | null {
    if (!this.isDev) return null
    const val = process.env[RAYBOX_CANDIDATE_ENV]
    return val ? val : null
  }

  private manifestPathInDir(dir: string): string {
    if (this.getCandidateDir() === dir || existsSync(join(dir, 'manifest.json'))) {
      return join(dir, 'manifest.json')
    }
    return join(dir, RAYBOX_MANIFEST_FILENAME)
  }

  private executablePathInDir(dir: string): string {
    if (this.getCandidateDir() === dir || existsSync(join(dir, RAYBOX_ASSET_NAME))) {
      return join(dir, RAYBOX_ASSET_NAME)
    }
    return join(dir, 'sing-box.exe')
  }

  private readBundledLockManifestSha256(): string | null {
    try {
      if (!existsSync(this.bundledLockPath)) return null
      const lock = JSON.parse(readFileSync(this.bundledLockPath, 'utf-8')) as RayBoxLock
      return isLowerHex(lock.manifestSha256, 64) ? lock.manifestSha256 : null
    } catch {
      return null
    }
  }

  private async validateRuntimeIdentity(
    manifestPath: string,
    executablePath: string,
    allowDirty: boolean
  ): Promise<RayBoxManifest> {
    const manifest = await validateRayboxManifest(manifestPath, executablePath, allowDirty)
    if (allowDirty || existsSync(join(dirname(manifestPath), RAYBOX_RECEIPT_FILENAME))) {
      return manifest
    }
    const hash = sha256Buffer(readFileSync(manifestPath))
    const bundledHash = this.readBundledLockManifestSha256()
    if (!bundledHash || hash !== bundledHash) {
      throw new Error('RayBox 缺少经官方 release 校验的来源记录，请重新安装内核')
    }
    return manifest
  }

  async resolveKernelDir(): Promise<string> {
    const candidate = this.getCandidateDir()
    if (candidate) {
      try {
        await validateRayboxManifest(
          join(candidate, 'manifest.json'),
          join(candidate, RAYBOX_ASSET_NAME),
          true
        )
        return candidate
      } catch {
        throw new Error('配置的 RayBox Windows 本地候选未通过 manifest 校验')
      }
    }

    for (const baseDir of [this.dataKernelDir, this.bundleKernelDir]) {
      const installed = join(baseDir, RAYBOX_INSTALL_DIR)
      if (existsSync(installed)) {
        await this.validateRuntimeIdentity(
          join(installed, RAYBOX_MANIFEST_FILENAME),
          join(installed, RAYBOX_ASSET_NAME),
          false
        )
        return installed
      }
      if (existsSync(join(baseDir, RAYBOX_MANIFEST_FILENAME))) {
        await this.validateRuntimeIdentity(
          join(baseDir, RAYBOX_MANIFEST_FILENAME),
          join(baseDir, 'sing-box.exe'),
          false
        )
        return baseDir
      }
    }

    throw new Error('未检测到通过来源校验的 RayBox 内核，请到【设置 → 内核】安装。')
  }

  async resolveKernelPath(): Promise<string> {
    const dir = await this.resolveKernelDir()
    return this.executablePathInDir(dir)
  }

  getKernelPath(_isAlpha = false): string {
    const candidate = this.getCandidateDir()
    if (candidate && existsSync(join(candidate, RAYBOX_ASSET_NAME))) {
      return join(candidate, RAYBOX_ASSET_NAME)
    }

    const dataInstalledExe = join(this.dataKernelDir, RAYBOX_INSTALL_DIR, RAYBOX_ASSET_NAME)
    if (existsSync(dataInstalledExe)) return dataInstalledExe

    const bundleInstalledExe = join(this.bundleKernelDir, RAYBOX_INSTALL_DIR, RAYBOX_ASSET_NAME)
    if (existsSync(bundleInstalledExe)) return bundleInstalledExe

    return dataInstalledExe
  }

  getPreferredOpenDirectory(): string {
    const candidate = this.getCandidateDir()
    if (candidate && existsSync(candidate)) return candidate
    const dataInstalled = join(this.dataKernelDir, RAYBOX_INSTALL_DIR)
    if (existsSync(dataInstalled)) return dataInstalled
    const bundleInstalled = join(this.bundleKernelDir, RAYBOX_INSTALL_DIR)
    if (existsSync(bundleInstalled)) return bundleInstalled
    return this.dataKernelDir
  }

  private async readKernelVersionOutput(kernelPath: string): Promise<KernelVersion | null> {
    if (!existsSync(kernelPath)) return null
    try {
      const { stdout } = await execFileAsync(kernelPath, ['version'], {
        windowsHide: true,
        timeout: 10000
      })
      const lines = stdout.split(/\r?\n/)
      const versionLine = lines.find(l => l.toLowerCase().includes('version'))
      const version = versionLine ? versionLine.trim().split(/\s+/).pop() || 'unknown' : 'unknown'
      return {
        version,
        versionDetail: stdout.trim(),
        isAlpha: false
      }
    } catch (err) {
      this.log(`Failed to get kernel version from ${kernelPath}: ${err}`)
      return null
    }
  }

  async getLocalVersion(_isAlpha = false): Promise<KernelVersion | null> {
    try {
      const kernelPath = await this.resolveKernelPath()
      return await this.readKernelVersionOutput(kernelPath)
    } catch {
      return null
    }
  }

  async getCapabilities(_isAlpha = false): Promise<KernelCapabilities> {
    try {
      const kernelPath = await this.resolveKernelPath()
      const dir = dirname(kernelPath)
      const manifest = await validateRayboxManifest(
        this.manifestPathInDir(dir),
        kernelPath,
        this.getCandidateDir() !== null
      )
      const local = await this.readKernelVersionOutput(kernelPath)
      const supportsNaive =
        manifest.buildTags.includes('with_naive_outbound') &&
        kernelSupportFileAvailable(kernelPath, 'libcronet.dll')
      return {
        version: local?.version || manifest.version,
        supportsNaive,
        supportsIcmpProxy: true,
        supportsBypassAction: true
      }
    } catch {
      return {
        version: 'unknown',
        supportsNaive: false,
        supportsIcmpProxy: true,
        supportsBypassAction: true
      }
    }
  }

  async getInstalledVersions(): Promise<InstalledKernel[]> {
    const versions: InstalledKernel[] = []
    try {
      const kernelPath = await this.resolveKernelPath()
      const info = await this.readKernelVersionOutput(kernelPath)
      if (info) {
        versions.push({
          version: info.version,
          versionDetail: info.versionDetail,
          isBackup: false,
          path: kernelPath
        })
      }
    } catch {
      // ignore if not installed
    }

    const backupDir = join(this.dataKernelDir, RAYBOX_BACKUP_DIR)
    const backupExe = join(backupDir, RAYBOX_ASSET_NAME)
    try {
      await this.validateRuntimeIdentity(
        join(backupDir, RAYBOX_MANIFEST_FILENAME),
        backupExe,
        false
      )
      const info = await this.readKernelVersionOutput(backupExe)
      if (info) {
        versions.push({
          version: info.version,
          versionDetail: info.versionDetail,
          isBackup: true,
          path: backupExe
        })
      }
    } catch {
      // ignore invalid or absent backup
    }

    return versions
  }

  async getRemoteReleases(_includePrerelease = false): Promise<RemoteRelease[]> {
    const collected: RemoteRelease[] = []
    const seenTags = new Set<string>()

    for (let page = 1; page <= MAX_RELEASE_PAGES; page += 1) {
      const url = `${this.apiRoot}/releases?per_page=100&page=${page}`
      const pageData = await this.fetchGitHubJson<GithubRelease[]>(url)
      if (!Array.isArray(pageData)) {
        throw new Error('GitHub 版本数据格式无效')
      }

      for (const release of pageData) {
        const parsed = parseWindowsReleaseTag(release.tag_name || '')
        if (!parsed || release.draft || release.prerelease) continue
        validateReleaseAssets(release)
        if (!seenTags.has(release.tag_name)) {
          seenTags.add(release.tag_name)
          collected.push({
            version: parsed.version,
            tagName: release.tag_name,
            publishedAt: release.published_at || '',
            isPrerelease: false,
            downloadUrl: `${this.downloadBaseUrl}/releases/download/${release.tag_name}/${RAYBOX_ASSET_NAME}`,
            assetName: RAYBOX_ASSET_NAME
          })
        }
      }

      if (pageData.length < 100) {
        collected.sort((a, b) => compareSemverDesc(a.version, b.version))
        return collected
      }
    }

    throw new Error('GitHub 版本页数过多，无法确认最新正式版本')
  }

  private async resolveReleaseTarget(tagName: string): Promise<RayBoxDownloadTarget> {
    const parsed = parseWindowsReleaseTag(tagName)
    if (!parsed) {
      throw new Error('RayBox Windows release 标签不是语义版本')
    }

    const release = await this.fetchGitHubJson<GithubRelease>(
      `${this.apiRoot}/releases/tags/${encodeURIComponent(tagName)}`
    )
    if (release.draft || release.prerelease) {
      throw new Error('不是正式发布版本')
    }
    if (release.tag_name !== tagName) {
      throw new Error('RayBox release 标签响应不一致')
    }

    const assets = validateReleaseAssets(release)
    const refData = await this.fetchGitHubJson<any>(
      `${this.apiRoot}/git/ref/tags/${encodeURIComponent(tagName)}`
    )
    let obj = refData?.object
    let commit: string | null = null

    for (let depth = 0; depth < 8; depth += 1) {
      if (!obj || !isLowerHex(obj.sha, 40)) {
        throw new Error('RayBox Git ref SHA 无效')
      }
      if (obj.type === 'commit') {
        commit = obj.sha
        break
      }
      if (obj.type === 'tag') {
        const tagObj = await this.fetchGitHubJson<any>(
          `${this.apiRoot}/git/tags/${obj.sha}`
        )
        obj = tagObj?.object
      } else {
        throw new Error('RayBox Git tag 没有指向 commit')
      }
    }

    if (!commit) {
      throw new Error('RayBox annotated tag 嵌套过深')
    }

    const manifestAsset = assets.get('manifest.json')!
    return {
      tag: tagName,
      version: parsed.version,
      commit,
      manifestSha256: releaseAssetDigest(manifestAsset),
      assets
    }
  }

  private validateTargetManifest(target: RayBoxDownloadTarget, rawBytes: Buffer): RayBoxManifest {
    const expected = target.assets.get('manifest.json')!
    if (rawBytes.length !== expected.size || sha256Buffer(rawBytes) !== target.manifestSha256) {
      throw new Error('RayBox manifest 的大小或 SHA-256 与 GitHub release 不一致')
    }

    const manifest = JSON.parse(rawBytes.toString('utf-8')) as RayBoxManifest
    if (!rayboxRepositoryMatches(manifest.repository)) {
      throw new Error('RayBox manifest 来源仓库错误')
    }
    if (
      manifest.commit !== target.commit ||
      manifest.fullHead !== target.commit ||
      manifest.dirty ||
      manifest.tag !== target.tag ||
      manifest.version !== target.version ||
      manifest.product !== 'RayBox'
    ) {
      throw new Error('RayBox manifest 与 release 的 tag/commit/版本不一致')
    }

    const manifestAssets = Object.entries(manifest.assets || {})
    if (manifestAssets.length + 1 !== target.assets.size) {
      throw new Error('RayBox manifest 与 release 资产集合不一致')
    }

    for (const [name, asset] of manifestAssets) {
      if (!isSafeAssetName(name) || name !== asset.file || name === 'manifest.json') {
        throw new Error('RayBox manifest 资产路径无效')
      }
      const remote = target.assets.get(name)
      if (!remote) {
        throw new Error(`release 缺少资产: ${name}`)
      }
      if (asset.size !== remote.size || asset.sha256 !== releaseAssetDigest(remote)) {
        throw new Error(`RayBox release 与 manifest 资产校验不一致: ${name}`)
      }
    }

    return manifest
  }

  private async checkCompatibility(directory: string, manifest: RayBoxManifest): Promise<void> {
    if (!manifest.buildTags.includes('with_clash_api')) {
      throw new Error('RayBox 缺少 Clash API 能力')
    }

    const executable = join(directory, RAYBOX_ASSET_NAME)
    const verResult = await runWithTimeout(executable, ['version'], directory, 15000)
    const lines = verResult.stdout.split(/\r?\n/).map(l => l.trim())
    const tagsLine = lines.find(l => l.startsWith('Tags: '))
    const actualTags = tagsLine ? tagsLine.slice('Tags: '.length).split(',').filter(Boolean) : []
    const expectedEnv = `Environment: ${manifest.go.replace(/^go version /, '')}`

    if (
      verResult.status !== 0 ||
      !lines.includes(`RayBox version ${manifest.version}`) ||
      !lines.includes(expectedEnv) ||
      actualTags.length !== manifest.buildTags.length ||
      manifest.buildTags.some(t => !actualTags.includes(t))
    ) {
      throw new Error('RayBox 实际版本、平台或功能与 manifest 不一致')
    }

    const compatConfigPath = join(directory, 'compatibility-check.json')
    writeFileSync(compatConfigPath, JSON.stringify(buildCompatibilityConfig()), 'utf-8')
    try {
      const checkResult = await runWithTimeout(
        executable,
        ['check', '-c', 'compatibility-check.json'],
        directory,
        20000
      )
      if (checkResult.status !== 0) {
        throw new Error(
          `RayBox 不兼容当前客户端配置: ${checkResult.stderr}${checkResult.stdout}`
        )
      }
    } finally {
      if (existsSync(compatConfigPath)) {
        try {
          unlinkSync(compatConfigPath)
        } catch {
          // ignore
        }
      }
    }
  }

  private async stageRelease(target: RayBoxDownloadTarget, stagedDir: string): Promise<void> {
    const manifestAsset = target.assets.get('manifest.json')!
    const manifestDest = join(stagedDir, RAYBOX_MANIFEST_FILENAME)
    const manifestUrl =
      manifestAsset.browser_download_url ||
      `${this.downloadBaseUrl}/releases/download/${target.tag}/manifest.json`

    await this.downloadVerifiedAsset(
      manifestUrl,
      manifestDest,
      manifestAsset.size,
      target.manifestSha256,
      false
    )

    const manifestBytes = readFileSync(manifestDest)
    const manifest = this.validateTargetManifest(target, manifestBytes)

    for (const [name, entry] of Object.entries(manifest.assets)) {
      const remoteAsset = target.assets.get(name)!
      const url =
        remoteAsset.browser_download_url ||
        `${this.downloadBaseUrl}/releases/download/${target.tag}/${name}`
      const reportProgress = name === RAYBOX_ASSET_NAME
      await this.downloadVerifiedAsset(url, join(stagedDir, name), entry.size, entry.sha256, reportProgress)
    }

    this.emit('downloadProgress', {
      phase: 'verifying',
      downloaded: 100,
      total: 100,
      percent: 100
    })

    await validateRayboxManifest(manifestDest, join(stagedDir, RAYBOX_ASSET_NAME), false)
    await this.checkCompatibility(stagedDir, manifest)

    const receipt: RayBoxSourceReceipt = {
      repository: RAYBOX_REPO_SLUG,
      tag: target.tag,
      commit: target.commit,
      version: target.version,
      manifestSha256: target.manifestSha256,
      assets: Object.fromEntries(Object.entries(manifest.assets).map(([k, v]) => [k, v.sha256]))
    }
    writeFileSync(join(stagedDir, RAYBOX_RECEIPT_FILENAME), JSON.stringify(receipt, null, 2), 'utf-8')
    await this.validateRuntimeIdentity(manifestDest, join(stagedDir, RAYBOX_ASSET_NAME), false)
  }

  private async ensureInitialRayboxBackup(previousDir: string | null): Promise<void> {
    const current = join(this.dataKernelDir, RAYBOX_INSTALL_DIR)
    if (existsSync(current) || !previousDir) return

    const manifest = await this.validateRuntimeIdentity(
      this.manifestPathInDir(previousDir),
      this.executablePathInDir(previousDir),
      false
    )

    const temporary = join(this.dataKernelDir, `raybox.initial-${randomUUID()}`)
    mkdirSync(temporary, { recursive: true })
    try {
      copyFileSync(this.manifestPathInDir(previousDir), join(temporary, RAYBOX_MANIFEST_FILENAME))
      for (const name of Object.keys(manifest.assets)) {
        const from =
          name === RAYBOX_ASSET_NAME ? this.executablePathInDir(previousDir) : join(previousDir, name)
        copyFileSync(from, join(temporary, name))
      }
      const prevReceipt = join(previousDir, RAYBOX_RECEIPT_FILENAME)
      if (existsSync(prevReceipt)) {
        copyFileSync(prevReceipt, join(temporary, RAYBOX_RECEIPT_FILENAME))
      }
      await this.validateRuntimeIdentity(
        join(temporary, RAYBOX_MANIFEST_FILENAME),
        join(temporary, RAYBOX_ASSET_NAME),
        false
      )
      renameSync(temporary, current)
    } finally {
      if (existsSync(temporary)) {
        rmSync(temporary, { recursive: true, force: true })
      }
    }
  }

  private replaceRayboxDirectory(stagedDir: string): void {
    const current = join(this.dataKernelDir, RAYBOX_INSTALL_DIR)
    const backup = join(this.dataKernelDir, RAYBOX_BACKUP_DIR)
    const retired = join(this.dataKernelDir, `raybox.retired-${randomUUID()}`)

    if (existsSync(backup)) {
      renameSync(backup, retired)
    }
    const hadCurrent = existsSync(current)
    if (hadCurrent) {
      try {
        renameSync(current, backup)
      } catch (err) {
        if (existsSync(retired)) {
          try {
            renameSync(retired, backup)
          } catch {
            // ignore
          }
        }
        throw err
      }
    }

    try {
      renameSync(stagedDir, current)
    } catch (err) {
      if (hadCurrent && existsSync(backup)) {
        try {
          renameSync(backup, current)
        } catch (restoreErr) {
          throw new Error(`安装失败: ${String(err)}；恢复失败: ${String(restoreErr)}`, { cause: restoreErr })
        }
      }
      if (existsSync(retired)) {
        try {
          renameSync(retired, backup)
        } catch {
          // ignore
        }
      }
      throw err
    }

    if (existsSync(retired)) {
      try {
        rmSync(retired, { recursive: true, force: true })
      } catch (cleanupErr) {
        this.log(`清理旧 RayBox 备份失败: ${cleanupErr}`)
      }
    }
  }

  private async rollbackRayboxDirectory(): Promise<void> {
    const current = join(this.dataKernelDir, RAYBOX_INSTALL_DIR)
    const backup = join(this.dataKernelDir, RAYBOX_BACKUP_DIR)

    await validateRayboxManifest(
      join(backup, RAYBOX_MANIFEST_FILENAME),
      join(backup, RAYBOX_ASSET_NAME),
      false
    )

    const temporary = join(this.dataKernelDir, `raybox.rollback-${randomUUID()}`)
    const hadCurrent = existsSync(current)
    if (hadCurrent) {
      renameSync(current, temporary)
    }

    try {
      renameSync(backup, current)
    } catch (err) {
      if (hadCurrent && existsSync(temporary)) {
        renameSync(temporary, current)
      }
      throw err
    }

    if (hadCurrent && existsSync(temporary)) {
      try {
        renameSync(temporary, backup)
      } catch (err) {
        try {
          renameSync(current, backup)
          renameSync(temporary, current)
        } catch {
          // ignore
        }
        throw err
      }
    }
  }

  async downloadKernel(tagNameOrRelease: string | RemoteRelease, _isAlpha = false): Promise<boolean> {
    if (this.operationBusy) {
      throw new Error('另一个内核更新或回滚正在进行')
    }
    this.operationBusy = true

    const tagName =
      typeof tagNameOrRelease === 'string' ? tagNameOrRelease : tagNameOrRelease.tagName
    const stagedDir = join(this.dataKernelDir, `raybox.download-${randomUUID()}`)

    try {
      const target = await this.resolveReleaseTarget(tagName)

      this.log(`Downloading ${target.tag}...`)
      this.emit('downloadProgress', {
        phase: 'downloading',
        downloaded: 0,
        total: 100,
        percent: 0
      })

      mkdirSync(stagedDir, { recursive: true })
      await this.stageRelease(target, stagedDir)

      await this.lifecycleHooks.cleanupTempProcesses?.()

      let previousDir: string | null = null
      try {
        previousDir = await this.resolveKernelDir()
      } catch {
        if (existsSync(join(this.dataKernelDir, RAYBOX_INSTALL_DIR))) {
          throw new Error('当前内核校验失败，请先修复后再更新，以保留可用备份')
        }
      }

      if (previousDir && previousDir !== this.getCandidateDir()) {
        const localManifest = await this.validateRuntimeIdentity(
          this.manifestPathInDir(previousDir),
          this.executablePathInDir(previousDir),
          false
        )
        if (
          strictSemverTriplet(localManifest.version) &&
          strictSemverTriplet(target.version) &&
          compareSemverDesc(target.version, localManifest.version) > 0
        ) {
          throw new Error('不能通过更新降级内核，请使用回退版本')
        }
      }

      await this.ensureInitialRayboxBackup(
        previousDir && previousDir !== this.getCandidateDir() ? previousDir : null
      )

      this.emit('downloadProgress', {
        phase: 'installing',
        downloaded: 100,
        total: 100,
        percent: 100
      })

      const { wasRunning } = (await this.lifecycleHooks.prepareForInstall?.()) ?? {
        wasRunning: false
      }

      try {
        this.replaceRayboxDirectory(stagedDir)
      } catch (replaceErr) {
        if (wasRunning) {
          const restored = await this.lifecycleHooks.restartAfterInstall?.()
          if (!restored?.success) {
            throw new Error(
              `安装失败: ${String(replaceErr)}；恢复旧内核连接失败: ${restored?.error || '未知错误'}`,
              { cause: replaceErr }
            )
          }
        }
        throw replaceErr
      }

      if (wasRunning) {
        const started = await this.lifecycleHooks.restartAfterInstall?.()
        if (!started?.success) {
          try {
            await this.lifecycleHooks.stopAfterFailedRestart?.()
          } catch {
            throw new Error('新内核启动失败且无法停止，未执行文件回滚')
          }
          try {
            await this.rollbackRayboxDirectory()
          } catch (rollbackErr) {
            throw new Error(
              `${started?.error || '新内核启动失败'}；旧内核恢复失败: ${String(rollbackErr)}`,
              { cause: rollbackErr }
            )
          }
          throw new Error(
            `${started?.error || '新内核启动失败'}；已恢复上一个 RayBox 工件，保持停止`
          )
        }
      }

      this.log(`RayBox kernel updated successfully to ${target.version}`)
      this.emit('downloadComplete')
      return true
    } catch (err: any) {
      const msg = err?.message || String(err)
      this.log(`Download failed: ${msg}`)
      this.emit('downloadError', msg)
      throw err instanceof Error ? err : new Error(msg)
    } finally {
      this.operationBusy = false
      if (existsSync(stagedDir)) {
        try {
          rmSync(stagedDir, { recursive: true, force: true })
        } catch {
          // ignore
        }
      }
    }
  }

  async rollback(_isAlpha = false): Promise<boolean> {
    if (this.operationBusy) {
      throw new Error('另一个内核更新或回滚正在进行')
    }
    this.operationBusy = true

    try {
      await this.lifecycleHooks.cleanupTempProcesses?.()
      if (this.lifecycleHooks.isRunning?.()) {
        throw new Error('请先停止连接再回滚 RayBox 内核')
      }

      const backupDir = join(this.dataKernelDir, RAYBOX_BACKUP_DIR)
      await this.validateRuntimeIdentity(
        join(backupDir, RAYBOX_MANIFEST_FILENAME),
        join(backupDir, RAYBOX_ASSET_NAME),
        false
      )

      await this.rollbackRayboxDirectory()
      return true
    } finally {
      this.operationBusy = false
    }
  }

  async canRollback(_isAlpha = false): Promise<boolean> {
    const backupDir = join(this.dataKernelDir, RAYBOX_BACKUP_DIR)
    try {
      await this.validateRuntimeIdentity(
        join(backupDir, RAYBOX_MANIFEST_FILENAME),
        join(backupDir, RAYBOX_ASSET_NAME),
        false
      )
      return true
    } catch {
      return false
    }
  }

  async clearCache(): Promise<{ success: boolean; freedBytes: number }> {
    let freedBytes = 0
    const getSize = (target: string): number => {
      if (!existsSync(target)) return 0
      const stat = statSync(target)
      if (!stat.isDirectory()) return stat.size
      return readdirSync(target).reduce((sum, item) => sum + getSize(join(target, item)), 0)
    }

    try {
      if (existsSync(this.cacheDir)) {
        for (const item of readdirSync(this.cacheDir)) {
          const itemPath = join(this.cacheDir, item)
          freedBytes += getSize(itemPath)
          rmSync(itemPath, { recursive: true, force: true })
        }
      }

      if (existsSync(this.dataKernelDir)) {
        for (const item of readdirSync(this.dataKernelDir)) {
          if (
            item.startsWith('raybox.download-') ||
            item.startsWith('raybox.initial-') ||
            item.startsWith('raybox.retired-') ||
            item.startsWith('raybox.rollback-')
          ) {
            const itemPath = join(this.dataKernelDir, item)
            freedBytes += getSize(itemPath)
            rmSync(itemPath, { recursive: true, force: true })
          }
        }
      }

      return { success: true, freedBytes }
    } catch {
      return { success: false, freedBytes: 0 }
    }
  }

  private async requestWithRedirects(
    url: string,
    acceptHeader: string,
    timeoutMs: number,
    redirectCount = 0
  ): Promise<http.IncomingMessage> {
    if (!isOfficialHttpsUrl(url, this.allowedTestOrigin)) {
      throw new Error('RayBox 下载地址不是官方 GitHub HTTPS 来源')
    }
    if (redirectCount > MAX_REDIRECTS) {
      throw new Error('RayBox 下载重定向次数过多')
    }

    return new Promise((resolve, reject) => {
      const client = url.startsWith('https://') ? https : http
      const req = client.get(
        url,
        {
          headers: {
            'User-Agent': 'KunBox-RayBox-Updater',
            Accept: acceptHeader
          },
          timeout: timeoutMs
        },
        res => {
          const status = res.statusCode || 0
          if ([301, 302, 303, 307, 308].includes(status) && res.headers.location) {
            const nextUrl = new URL(res.headers.location, url).toString()
            res.resume()
            this.requestWithRedirects(nextUrl, acceptHeader, timeoutMs, redirectCount + 1)
              .then(resolve)
              .catch(reject)
            return
          }
          if (status < 200 || status >= 300) {
            res.resume()
            reject(new Error(`GitHub 请求失败（HTTP ${status}），请稍后重试`))
            return
          }
          resolve(res)
        }
      )

      req.on('timeout', () => {
        req.destroy(new Error('检查或下载 RayBox 官方版本超时'))
      })
      req.on('error', err => reject(err))
    })
  }

  private async fetchGitHubJson<T>(url: string): Promise<T> {
    const res = await this.requestWithRedirects(url, 'application/vnd.github+json', 20000)
    return new Promise((resolve, reject) => {
      const chunks: Buffer[] = []
      let total = 0
      res.on('data', (chunk: Buffer) => {
        total += chunk.length
        if (total > MAX_METADATA_BYTES) {
          res.destroy()
          reject(new Error('GitHub 元数据超过大小限制'))
          return
        }
        chunks.push(chunk)
      })
      res.on('end', () => {
        try {
          resolve(JSON.parse(Buffer.concat(chunks).toString('utf-8')) as T)
        } catch (err) {
          reject(new Error(`GitHub 版本数据无效: ${String(err)}`))
        }
      })
      res.on('error', reject)
    })
  }

  private async downloadVerifiedAsset(
    url: string,
    destPath: string,
    expectedSize: number,
    expectedSha256: string,
    reportProgress: boolean
  ): Promise<void> {
    const res = await this.requestWithRedirects(url, 'application/octet-stream', 180000)
    const contentLengthHeader = res.headers['content-length']
    if (contentLengthHeader !== undefined) {
      const declared = parseInt(String(contentLengthHeader), 10)
      if (!Number.isNaN(declared) && declared !== expectedSize) {
        res.destroy()
        throw new Error('RayBox 下载长度与 release 声明不匹配')
      }
    }

    await new Promise<void>((resolve, reject) => {
      const hash = createHash('sha256')
      const out = createWriteStream(destPath)
      let downloaded = 0
      let finished = false

      const fail = (err: Error) => {
        if (finished) return
        finished = true
        res.destroy()
        out.close(() => {
          if (existsSync(destPath)) {
            try {
              unlinkSync(destPath)
            } catch {
              // ignore
            }
          }
          reject(err)
        })
      }

      res.on('data', (chunk: Buffer) => {
        downloaded += chunk.length
        if (downloaded > expectedSize || downloaded > MAX_KERNEL_DOWNLOAD_SIZE_BYTES) {
          fail(new Error('RayBox 下载超过声明大小'))
          return
        }
        hash.update(chunk)
        if (!out.write(chunk)) {
          res.pause()
          out.once('drain', () => res.resume())
        }
        if (reportProgress) {
          this.emit('downloadProgress', {
            phase: 'downloading',
            downloaded,
            total: expectedSize,
            percent: Math.min(100, Math.round((downloaded * 100) / Math.max(1, expectedSize)))
          })
        }
      })

      res.on('end', () => {
        if (finished) return
        out.end(() => {
          if (finished) return
          const actualSha = hash.digest('hex')
          if (downloaded !== expectedSize || actualSha !== expectedSha256) {
            fail(new Error('RayBox 下载文件大小或 SHA-256 不匹配'))
            return
          }
          finished = true
          resolve()
        })
      })

      res.on('error', err => fail(err))
      out.on('error', err => fail(err))
    })
  }

  private log(msg: string): void {
    this.logger(`[KernelManager] ${msg}`)
  }
}
