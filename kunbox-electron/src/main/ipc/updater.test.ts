import assert from 'node:assert/strict'
import test from 'node:test'
import { compareSemver, selectWindowsInstallerAsset } from './updaterUtils.ts'

test('compareSemver 正确比较语义化版本号', () => {
  assert.equal(compareSemver('v0.0.43', '0.0.24'), 1)
  assert.equal(compareSemver('0.0.10', '0.0.9'), 1)
  assert.equal(compareSemver('0.0.43', '0.0.43'), 0)
  assert.equal(compareSemver('0.0.24', '0.0.43'), -1)
  assert.equal(compareSemver('1.0.0', '0.9.9'), 1)
})

test('selectWindowsInstallerAsset 仅选中 NSIS 安装包并排除 portable 与 blockmap', () => {
  const asset = selectWindowsInstallerAsset([
    { name: 'KunBox-0.0.43-portable.exe', size: 100, browser_download_url: 'https://example.com/p.exe' },
    { name: 'KunBox-0.0.43-x64.exe.blockmap', size: 10, browser_download_url: 'https://example.com/b' },
    { name: 'KunBox-0.0.43-x64.exe', size: 200, browser_download_url: 'https://example.com/setup.exe' }
  ])
  assert.equal(asset?.name, 'KunBox-0.0.43-x64.exe')
})
