import { useCallback, useState } from 'react'
import type { Profile } from '@shared/types'

export function useProfiles() {
  const [profiles, setProfiles] = useState<Profile[]>([])

  const loadProfiles = useCallback(async () => {
    const list = await window.api.profile.list()
    setProfiles(list)
    return list
  }, [])

  return {
    profiles,
    setProfiles,
    loadProfiles,
  }
}
