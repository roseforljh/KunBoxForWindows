import { useCallback, useState } from 'react'
import type { Profile } from '@shared/types'

export function resolveActiveProfile(profiles: Profile[], activeProfileId: string | null): Profile | undefined {
  if (activeProfileId) {
    const matched = profiles.find((p) => p.id === activeProfileId)
    if (matched) return matched
  }
  return profiles.find((p) => p.enabled)
}

export function useProfiles() {
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null)

  const loadProfiles = useCallback(async () => {
    const [list, activeId] = await Promise.all([
      window.api.profile.list(),
      window.api.profile.getActive(),
    ])
    setProfiles(list)
    setActiveProfileId(activeId)
    return list
  }, [])

  return {
    profiles,
    activeProfileId,
    setProfiles,
    loadProfiles,
  }
}
