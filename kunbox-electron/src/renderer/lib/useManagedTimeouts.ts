import { useCallback, useEffect, useRef } from 'react'

export function useManagedTimeouts() {
  const timeoutIdsRef = useRef<number[]>([])

  const setManagedTimeout = useCallback((callback: () => void, delay: number) => {
    const id = window.setTimeout(() => {
      timeoutIdsRef.current = timeoutIdsRef.current.filter((v) => v !== id)
      callback()
    }, delay)

    timeoutIdsRef.current.push(id)
    return id
  }, [])

  const clearManagedTimeout = useCallback((id: number) => {
    clearTimeout(id)
    timeoutIdsRef.current = timeoutIdsRef.current.filter((v) => v !== id)
  }, [])

  useEffect(() => {
    return () => {
      timeoutIdsRef.current.forEach((id) => clearTimeout(id))
      timeoutIdsRef.current = []
    }
  }, [])

  return { setManagedTimeout, clearManagedTimeout }
}
