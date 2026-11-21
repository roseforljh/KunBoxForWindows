import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const rootDir = path.resolve(fileURLToPath(import.meta.url), '../..')
const electronDir = path.join(rootDir, 'kunbox-electron')
const releaseDir = path.join(electronDir, 'release')

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

cleanIntermediates()

const result = spawnSync('npm run build:win', {
  cwd: electronDir,
  stdio: 'inherit',
  shell: true
})

if (result.status !== 0) {
  process.exit(result.status ?? 1)
}

cleanIntermediates()

console.log('\n=== 编译成功 ===')
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
