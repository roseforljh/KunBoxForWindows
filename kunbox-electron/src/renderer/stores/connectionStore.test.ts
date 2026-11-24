import { describe, it, beforeEach } from 'node:test'
import assert from 'node:assert/strict'
import { useConnectionStore } from './connectionStore'

describe('useConnectionStore trafficHistory', () => {
  beforeEach(() => {
    useConnectionStore.setState({
      state: 'idle',
      traffic: null,
      trafficHistory: [],
      lastError: null,
      needsRestart: false
    })
  })

  it('retains up to 40 traffic samples continuously and resets only when idle or error', () => {
    const store = useConnectionStore.getState()
    store.setState('connected')

    for (let i = 1; i <= 45; i += 1) {
      useConnectionStore.getState().setTraffic({
        downloadSpeed: i * 100,
        uploadSpeed: i * 10,
        downloadTotal: i * 1000,
        uploadTotal: i * 100,
        duration: i
      })
    }

    const { trafficHistory } = useConnectionStore.getState()
    assert.equal(trafficHistory.length, 40)
    assert.deepEqual(trafficHistory[0], { download: 600, upload: 60 })
    assert.deepEqual(trafficHistory[39], { download: 4500, upload: 450 })

    useConnectionStore.getState().setState('connected')
    assert.equal(useConnectionStore.getState().trafficHistory.length, 40)

    useConnectionStore.getState().setState('idle')
    assert.equal(useConnectionStore.getState().trafficHistory.length, 0)
    assert.equal(useConnectionStore.getState().traffic, null)
  })
})
