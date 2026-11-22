import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const rootDir = path.resolve(fileURLToPath(import.meta.url), '../..')
const electronDir = path.join(rootDir, 'kunbox-electron')
const releaseDir = path.join(electronDir, 'release')
const fastMode = process.argv.includes('--fast') || process.env.FAST_BUILD === '1'

const viteCli = path.join(electronDir, 'node_modules', 'electron-vite', 'bin', 'electron-vite.js')
const builderCli = path.join(electronDir, 'node_modules', 'electron-builder', 'cli.js')

// 清理中间解包目录与元数据文件，只保留最终安装程序
function cleanIntermediates() {
  if (!fs.existsSync(releaseDir)) return

  const unpackedDir = path.join(releaseDir, 'win-unpacked')
  if (fs.existsSync(unpackedDir)) {
    fs.rmSync(unpackedDir, { recursive: true, force: true })
  }

  for (const item of fs.readdirSync(releaseDir)) {
    if (item.endsWith('.yaml') || item.endsWith('.yml') || item.endsWith('.blockmap')) {
      fs.rmSync(path.join(releaseDir, item), { force: true })
    }
  }
}

function runStep(label, args) {
  const start = performance.now()
  const result = spawnSync(process.execPath, args, {
    cwd: electronDir,
    stdio: 'inherit'
  })
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
  const sec = ((performance.now() - start) / 1000).toFixed(1)
  console.log(`[${label}] 完成 (${sec}s)`)
}

const totalStart = performance.now()

// 直接调用 Node CLI 入口，跳过 cmd.exe 与 npm 多层启动开销
runStep('electron-vite', [viteCli, 'build'])

const builderArgs = [builderCli, '--win', '--publish', 'never']
if (fastMode) {
  builderArgs.push('-c.compression=store')
  console.log('[electron-builder] 已启用 --fast 极速模式（跳过 7z LZMA 固实压缩）')
}
runStep('electron-builder', builderArgs)

cleanIntermediates()

const totalSec = ((performance.now() - totalStart) / 1000).toFixed(1)
console.log(`\n=== 编译成功（总耗时 ${totalSec}s）===`)
console.log(`产物位于: ${releaseDir}`)

if (fs.existsSync(releaseDir)) {
  const exes = fs
    .readdirSync(releaseDir)
    .filter((f) => f.endsWith('.exe'))
    .sort()
  console.log('生成的可执行程序:')
  for (const exe of exes) {
    console.log(` - ${path.join(releaseDir, exe)}`)
  }
}
