import { Component, ReactNode } from 'react'
import { AlertTriangle, RefreshCw } from 'lucide-react'

interface ErrorBoundaryProps {
  children: ReactNode
}

interface ErrorBoundaryState {
  hasError: boolean
  errorMessage: string
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = {
    hasError: false,
    errorMessage: ''
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return {
      hasError: true,
      errorMessage: error.message || '发生未知错误'
    }
  }

  componentDidCatch(error: Error, errorInfo: unknown) {
    console.error('UI crashed:', error, errorInfo)
  }

  handleReload = () => {
    window.location.reload()
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="min-h-screen flex items-center justify-center px-6" style={{ backgroundColor: 'var(--bg-primary)' }}>
          <div className="glass-card rounded-2xl p-8 max-w-md w-full text-center space-y-4">
            <div className="w-14 h-14 rounded-2xl bg-red-500/10 mx-auto flex items-center justify-center">
              <AlertTriangle className="w-7 h-7 text-red-400" />
            </div>
            <div className="space-y-2">
              <h1 className="text-xl font-semibold text-[var(--text-primary)]">界面发生错误</h1>
              <p className="text-sm text-[var(--text-muted)] break-words">{this.state.errorMessage}</p>
            </div>
            <button
              onClick={this.handleReload}
              className="inline-flex items-center gap-2 px-4 py-2 rounded-xl bg-[var(--accent-primary)] text-white hover:opacity-90 transition-opacity"
            >
              <RefreshCw className="w-4 h-4" />
              重新加载
            </button>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}
