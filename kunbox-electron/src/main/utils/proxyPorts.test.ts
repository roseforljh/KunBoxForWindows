import assert from 'node:assert/strict'
import { createServer, type AddressInfo } from 'node:net'
import { test } from 'node:test'
import { findAvailablePort, proxyListenHost, resolveProxyPorts } from './proxyPorts'

for (const allowLan of [false, true]) {
  test(`替换占用端口、保留可用端口（allowLan=${allowLan}）`, async (t) => {
    const host = proxyListenHost(allowLan)
    const owner = createServer()
    await new Promise<void>((resolve, reject) => {
      owner.once('error', reject)
      owner.listen(0, '127.0.0.1', resolve)
    })
    t.after(() => owner.close())
    const busy = (owner.address() as AddressInfo).port
    const apiPort = await findAvailablePort(host, [busy])
    const free = await findAvailablePort(host, [busy, apiPort])

    const localConflict = await resolveProxyPorts({ localPort: busy, socksPort: free, allowLan }, apiPort)
    assert.equal(localConflict.socksPort, free)
    assert.ok(![busy, apiPort, free].includes(localConflict.localPort))

    const socksConflict = await resolveProxyPorts({ localPort: free, socksPort: busy, allowLan }, apiPort)
    assert.equal(socksConflict.localPort, free)
    assert.ok(![busy, apiPort, free].includes(socksConflict.socksPort))

    const shared = await resolveProxyPorts({ localPort: busy, socksPort: busy, allowLan }, apiPort)
    assert.equal(shared.localPort, shared.socksPort)
    assert.notEqual(shared.localPort, busy)

    const internalConflict = await resolveProxyPorts({ localPort: apiPort, socksPort: 0, allowLan }, apiPort)
    assert.notEqual(internalConflict.localPort, apiPort)
    assert.equal(internalConflict.socksPort, 0)
  })
}
