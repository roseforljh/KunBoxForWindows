import type { API } from '../../shared/tauri-api'
import type { HealthEvent, HealthEventKind, HealthStatus } from '../../shared/types'
import { HEALTH_EVENT_TOAST_TYPES } from '../components/Dashboard'
import { useNodesStore } from './nodesStore'

type HealthToastType = 'success' | 'warning' | 'error'

const toastTypes: Record<HealthEventKind, HealthToastType> = HEALTH_EVENT_TOAST_TYPES

function assertHealthEventApi(api: API) {
  const unlisten = api.singbox.onHealthEvent((event: HealthEvent) => {
    const kind: HealthEventKind = event.kind
    void kind
  })
  unlisten()
}

function assertNodeHealthStore() {
  const store = useNodesStore.getState()
  store.setNodeHealth('node-a', 'healthy')
  const nodeHealth: Record<string, HealthStatus> = store.nodeHealth
  void nodeHealth
}

void toastTypes
void assertHealthEventApi
void assertNodeHealthStore
