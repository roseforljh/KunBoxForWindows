export interface GitHubAsset {
  name: string
  size: number
  browser_download_url: string
}

// 比较语义化版本号：a > b 返回 1，a < b 返回 -1，相等返回 0
export function compareSemver(a: string, b: string): number {
  const pa = a.replace(/^v/i, '').split('-')[0].split('.').map((n) => Number.parseInt(n, 10) || 0)
  const pb = b.replace(/^v/i, '').split('-')[0].split('.').map((n) => Number.parseInt(n, 10) || 0)
  const len = Math.max(pa.length, pb.length)
  for (let i = 0; i < len; i++) {
    const na = pa[i] ?? 0
    const nb = pb[i] ?? 0
    if (na > nb) return 1
    if (na < nb) return -1
  }
  return 0
}

// 从 Release 附件中挑选 Windows NSIS 安装包（排除便携版和签名/校验文件）
export function selectWindowsInstallerAsset(assets: GitHubAsset[] = []): GitHubAsset | undefined {
  return assets.find((asset) => {
    const lower = asset.name.toLowerCase()
    return lower.endsWith('.exe') && !lower.includes('portable')
  })
}
