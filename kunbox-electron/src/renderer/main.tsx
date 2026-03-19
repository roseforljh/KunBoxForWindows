import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './styles/globals.css'

// Detect if running in Tauri
const isTauri = '__TAURI_INTERNALS__' in window
const hasElectronApi = !!(window as any).api

async function initApp() {
  if (isTauri) {
    // Dynamically import and initialize Tauri API before rendering
    const { initTauriApi } = await import('../shared/tauri-api')
    initTauriApi()
    console.log('[Tauri] API initialized')
  } else if (hasElectronApi) {
    console.log('[Electron] Using preload API')
  } else {
    // Browser-only mode: load mock API for development/QA testing
    const { initMockApi } = await import('../shared/mock-api')
    initMockApi()
    console.log('[Browser] Mock API initialized (dev/QA mode)')
  }

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
}

initApp().catch(console.error)
