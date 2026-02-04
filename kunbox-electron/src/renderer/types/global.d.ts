import type { API } from '../preload/index'

declare global {
  interface Window {
    api: API
  }
}
