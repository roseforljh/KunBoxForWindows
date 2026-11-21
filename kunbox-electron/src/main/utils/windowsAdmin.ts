import { execFile } from 'node:child_process'
import { join } from 'node:path'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)
const waitArg = '--kunbox-wait-for-exit='

function runPowerShell(script: string, timeout?: number) {
  const executable = join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe')
  const encoded = Buffer.from(script, 'utf16le').toString('base64')
  return execFileAsync(executable, ['-NoProfile', '-NonInteractive', '-EncodedCommand', encoded], {
    windowsHide: true,
    timeout
  })
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

export async function launchAsAdmin(executable: string, appPath?: string): Promise<void> {
  if (process.platform !== 'win32') throw new Error('管理员重启仅在 Windows 上可用')
  const quote = (value: string) => `'${value.replaceAll("'", "''")}'`
  const args = appPath ? `"${appPath}" ${waitArg}${process.pid}` : `${waitArg}${process.pid}`
  await runPowerShell(`Start-Process -FilePath ${quote(executable)} -ArgumentList ${quote(args)} -Verb RunAs -ErrorAction Stop`)
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
