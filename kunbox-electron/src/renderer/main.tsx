import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { ErrorBoundary } from './components/ErrorBoundary'
import './styles/globals.css'

const hasElectronApi = !!(window as any).api

async function initApp() {
  if (hasElectronApi) {
    console.log('[Electron] Using preload API')
  } else {
    // Browser-only mode: load mock API for development/QA testing
    const { initMockApi } = await import('../shared/mock-api')
    initMockApi()
    console.log('[Browser] Mock API initialized (dev/QA mode)')
  }

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </React.StrictMode>
  )
}

initApp().catch(console.error)
