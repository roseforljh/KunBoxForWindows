import { execFile } from 'node:child_process'
import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs'
import net from 'node:net'
import { dirname, join } from 'node:path'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)
const waitArg = '--kunbox-wait-for-exit='
export const AUTOSTART_ARG = '--autostart'
export const AUTOSTART_TASK_NAME = 'KunBoxAutoStart'

function runPowerShell(script: string, timeout?: number) {
  const executable = join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe')
  const encoded = Buffer.from(script, 'utf16le').toString('base64')
  return execFileAsync(executable, ['-NoProfile', '-NonInteractive', '-EncodedCommand', encoded], {
    windowsHide: true,
    timeout
  })
}

export function isAutoStartLaunch(argv = process.argv): boolean {
  return argv.includes(AUTOSTART_ARG)
}

export async function isRunningAsAdmin(): Promise<boolean> {
  if (process.platform !== 'win32') return false
  try {
    const { stdout } = await runPowerShell(`
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
`, 5000)
    return stdout.trim() === 'True'
  } catch {
    return false
  }
}

export async function launchAsAdmin(executable: string, appPath?: string, argv = process.argv): Promise<void> {
  if (process.platform !== 'win32') throw new Error('管理员重启仅在 Windows 上可用')
  const quote = (value: string) => `'${value.replaceAll("'", "''")}'`
  const extra = isAutoStartLaunch(argv) ? ` ${AUTOSTART_ARG}` : ''
  const args = appPath
    ? `"${appPath}" ${waitArg}${process.pid}${extra}`
    : `${waitArg}${process.pid}${extra}`
  await runPowerShell(`Start-Process -FilePath ${quote(executable)} -ArgumentList ${quote(args)} -Verb RunAs -ErrorAction Stop`)
}

export async function requestStartupElevation(
  requireAdmin: boolean,
  launch: () => Promise<void>,
  checkAdmin = isRunningAsAdmin,
  argv = process.argv
): Promise<boolean> {
  if (process.platform !== 'win32' || !requireAdmin || argv.some((arg) => arg.startsWith(waitArg)) || await checkAdmin()) {
    return false
  }
  await launch()
  return true
}

export async function waitForAdminRestart(argv = process.argv, timeoutSeconds = 30): Promise<void> {
  if (process.platform !== 'win32') return
  const arg = argv.find((value) => value.startsWith(waitArg))
  const previousPid = Number(arg?.slice(waitArg.length))
  if (!Number.isInteger(previousPid) || previousPid <= 0 || previousPid === process.pid) return

  // The elevated instance must not acquire the single-instance lock before the old process quits.
  await runPowerShell(`
$previous = Get-Process -Id ${previousPid} -ErrorAction SilentlyContinue
if ($previous) {
  try { Wait-Process -InputObject $previous -Timeout ${timeoutSeconds} -ErrorAction Stop }
  catch { if (-not $previous.HasExited) { exit 1 } }
}
exit 0
`, (timeoutSeconds + 5) * 1000)
}

// 通过本地回环端口绕过 Windows UIPI，允许普通权限进程唤醒管理员权限运行的主窗口
export function startActivationServer(portFilePath: string, onActivate: () => void): () => void {
  const server = net.createServer((socket) => {
    socket.destroy()
    onActivate()
  })

  server.on('error', () => {
    // 忽略唤醒服务监听异常，避免影响主进程启动
  })

  server.listen(0, '127.0.0.1', () => {
    const address = server.address()
    if (address && typeof address === 'object') {
      try {
        mkdirSync(dirname(portFilePath), { recursive: true })
        writeFileSync(portFilePath, String(address.port), 'utf-8')
      } catch {
        // 忽略端口文件写入失败
      }
    }
  })

  return () => {
    server.close()
    try {
      unlinkSync(portFilePath)
    } catch {
      // 忽略清理失败
    }
  }
}

export function notifyRunningInstance(portFilePath: string, timeoutMs = 1000): Promise<boolean> {
  let port = 0
  try {
    port = Number(readFileSync(portFilePath, 'utf-8').trim())
  } catch {
    return Promise.resolve(false)
  }

  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    return Promise.resolve(false)
  }

  return new Promise((resolve) => {
    let settled = false
    const finish = (ok: boolean) => {
      if (settled) return
      settled = true
      socket.destroy()
      resolve(ok)
    }

    const socket = net.createConnection({ host: '127.0.0.1', port }, () => {
      finish(true)
    })
    socket.setTimeout(timeoutMs, () => finish(false))
    socket.on('error', () => finish(false))
  })
}

export type AutoStartStrategy = 'task' | 'registry' | 'disabled'

function runSchtasks(args: string[]): Promise<void> {
  const executable = join(process.env.SystemRoot || 'C:\\Windows', 'System32/schtasks.exe')
  return execFileAsync(executable, args, { windowsHide: true, timeout: 8000 }).then(() => undefined)
}

/** 同步 Windows 开机自启：优先创建最高权限计划任务（开机免 UAC），无管理员权限时回退原生登录项；关闭时清理两者。 */
export async function syncWindowsAutoStart(
  enabled: boolean,
  executable: string,
  setLoginItem: (openAtLogin: boolean, args: string[]) => void,
  execSchtasks: (args: string[]) => Promise<void> = runSchtasks
): Promise<AutoStartStrategy> {
  if (!enabled) {
    setLoginItem(false, [])
    try {
      await execSchtasks(['/Delete', '/TN', AUTOSTART_TASK_NAME, '/F'])
    } catch {
      // 计划任务不存在或无权删除时忽略
    }
    return 'disabled'
  }

  try {
    await execSchtasks([
      '/Create',
      '/F',
      '/SC', 'ONLOGON',
      '/RL', 'HIGHEST',
      '/TN', AUTOSTART_TASK_NAME,
      '/TR', `"${executable}" ${AUTOSTART_ARG}`
    ])
    // 已由最高权限计划任务接管，清理注册表启动项避免开机重复启动
    setLoginItem(false, [])
    return 'task'
  } catch {
    setLoginItem(true, [AUTOSTART_ARG])
    return 'registry'
  }
}
