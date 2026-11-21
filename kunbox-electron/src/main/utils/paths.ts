import { existsSync, mkdirSync } from 'fs'
import { join } from 'path'

// 数据目录不能放在安装目录内，NSIS 安装/升级会 RMDir /r 整个安装目录
const D_DRIVE_DATA_DIR = 'D:\\KunBoxData'

// 优先使用 D:\KunBoxData，若不存在 D 盘则回退至 %APPDATA%\KunBox
export function getDataDir(): string {
  const dir =
    process.platform === 'win32' && existsSync('D:\\')
      ? D_DRIVE_DATA_DIR
      : join(process.env.APPDATA || '', 'KunBox')

  try {
    mkdirSync(dir, { recursive: true })
    return dir
  } catch {
    // D 盘不可写时退回用户目录，避免启动即崩溃
    const fallback = join(process.env.APPDATA || '', 'KunBox')
    mkdirSync(fallback, { recursive: true })
    return fallback
  }
}
