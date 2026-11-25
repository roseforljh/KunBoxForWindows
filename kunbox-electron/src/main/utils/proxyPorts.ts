import { createServer, type AddressInfo } from 'node:net'
import type { AppSettings } from '../../shared/types'

export function proxyListenHost(allowLan: boolean): string {
  return allowLan ? '0.0.0.0' : '127.0.0.1'
}

function probePort(host: string, port: number): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.unref()
    server.once('error', reject)
    server.listen({ host, port, exclusive: true }, () => {
      const { port: boundPort } = server.address() as AddressInfo
      server.close(() => resolve(boundPort))
    })
  })
}

export async function findAvailablePort(host: string, excluded: readonly number[]): Promise<number> {
  // ponytail: the OS picks the port, but the probe must close before RayBox can bind it.
  for (let attempt = 0; attempt < 10; attempt++) {
    const port = await probePort(host, 0)
    if (!excluded.includes(port)) return port
  }
  throw new Error('找不到可用的代理端口')
}

async function canListen(host: string, port: number): Promise<boolean> {
  try {
    await probePort(host, port)
    return true
  } catch (error) {
    // Windows reports excluded TCP ranges as EACCES even when no process owns the port.
    if (['EADDRINUSE', 'EACCES'].includes((error as NodeJS.ErrnoException).code ?? '')) return false
    throw error
  }
}

export async function resolveProxyPorts(
  { localPort, socksPort, allowLan }: Pick<AppSettings, 'localPort' | 'socksPort' | 'allowLan'>,
  apiPort: number
): Promise<{ localPort: number; socksPort: number }> {
  const host = proxyListenHost(allowLan)
  const excluded = [apiPort, localPort, socksPort]
  const keep = async (port: number) => port <= 0 || (port !== apiPort && await canListen(host, port))
  const samePort = socksPort === localPort
  const [localOk, socksOk] = await Promise.all([
    keep(localPort),
    samePort ? Promise.resolve(true) : keep(socksPort)
  ])
  const newLocal = localOk ? localPort : await findAvailablePort(host, excluded)
  const newSocks = samePort ? newLocal
    : socksOk ? socksPort : await findAvailablePort(host, [...excluded, newLocal])
  return { localPort: newLocal, socksPort: newSocks }
}

/** 检测 Clash API 端口是否可用，被占用时自动避让到空闲端口。 */
export async function resolveApiPort(preferredPort: number): Promise<number> {
  if (await canListen('127.0.0.1', preferredPort)) return preferredPort
  return findAvailablePort('127.0.0.1', [preferredPort])
}
