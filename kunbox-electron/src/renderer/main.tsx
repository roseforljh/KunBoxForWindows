import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './styles/globals.css'

// Detect if running in Tauri
const isTauri = '__TAURI_INTERNALS__' in window

async function initApp() {
  if (isTauri) {
    // Dynamically import and initialize Tauri API before rendering
    const { initTauriApi } = await import('../shared/tauri-api')
    initTauriApi()
    console.log('[Tauri] API initialized')
  } else {
    console.log('[Electron] Using preload API')
  }

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
}

initApp().catch(console.error)
