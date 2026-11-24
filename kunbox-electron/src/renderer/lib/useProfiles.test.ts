import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { Profile } from '../../shared/types'
import { resolveActiveProfile } from './useProfiles'

const profiles: Profile[] = [
  { id: 'p-1', name: 'LXY', url: '', nodeCount: 53, enabled: true, autoUpdateInterval: 0, dnsPreResolve: false, dnsServer: null },
  { id: 'p-2', name: 'roybox', url: '', nodeCount: 13, enabled: true, autoUpdateInterval: 0, dnsPreResolve: false, dnsServer: null }
]

test('resolveActiveProfile prefers activeProfileId over first enabled profile', () => {
  assert.equal(resolveActiveProfile(profiles, 'p-2')?.name, 'roybox')
  assert.equal(resolveActiveProfile(profiles, 'p-1')?.name, 'LXY')
  assert.equal(resolveActiveProfile(profiles, null)?.name, 'LXY')
  assert.equal(resolveActiveProfile(profiles, 'missing')?.name, 'LXY')
})
