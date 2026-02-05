import { createContext, useContext, useState, useCallback, ReactNode } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Check, XCircle, AlertCircle, Info, RefreshCw } from 'lucide-react'
import { useConnectionStore } from '../../stores/connectionStore'

type ToastType = 'success' | 'error' | 'warning' | 'info'

interface ToastAction {
  label: string
  onClick: () => void
}

interface ToastMessage {
  id: number
  message: string
  type: ToastType
  action?: ToastAction
}

interface ToastContextValue {
  showToast: (message: string, type?: ToastType, action?: ToastAction) => void
  showRestartToast: (message: string) => void
  success: (message: string) => void
  error: (message: string) => void
  warning: (message: string) => void
  info: (message: string) => void
}

const ToastContext = createContext<ToastContextValue | null>(null)

export function useToast() {
  const context = useContext(ToastContext)
  if (!context) {
    throw new Error('useToast must be used within a ToastProvider')
  }
  return context
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastMessage[]>([])
  const [isRestarting, setIsRestarting] = useState(false)

  const showToast = useCallback((message: string, type: ToastType = 'info', action?: ToastAction) => {
    const id = Date.now()
    setToasts(prev => [...prev, { id, message, type, action }])
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id))
    }, action ? 8000 : 3000)
  }, [])

  const removeToast = useCallback((id: number) => {
    setToasts(prev => prev.filter(t => t.id !== id))
  }, [])

  const success = useCallback((message: string) => showToast(message, 'success'), [showToast])
  const error = useCallback((message: string) => showToast(message, 'error'), [showToast])
  const warning = useCallback((message: string) => showToast(message, 'warning'), [showToast])
  const info = useCallback((message: string) => showToast(message, 'info'), [showToast])

  const getIcon = (type: ToastType) => {
    switch (type) {
      case 'success':
        return <Check className="w-4 h-4 text-emerald-400" />
      case 'error':
        return <XCircle className="w-4 h-4 text-red-400" />
      case 'warning':
        return <AlertCircle className="w-4 h-4 text-amber-400" />
      case 'info':
        return <Info className="w-4 h-4 text-blue-400" />
    }
  }

  const getBgColor = (type: ToastType) => {
    switch (type) {
      case 'success':
        return 'bg-emerald-500/10'
      case 'error':
        return 'bg-red-500/10'
      case 'warning':
        return 'bg-amber-500/10'
      case 'info':
        return 'bg-blue-500/10'
    }
  }

  return (
    <ToastContext.Provider value={{ showToast, showRestartToast: () => {}, success, error, warning, info }}>
      <ToastProviderInner 
        showToast={showToast} 
        isRestarting={isRestarting}
        setIsRestarting={setIsRestarting}
      >
        {children}
      </ToastProviderInner>
      <div className="fixed bottom-6 left-1/2 -translate-x-1/2 z-[9999] flex flex-col-reverse gap-2 pointer-events-none">
        <AnimatePresence>
          {toasts.map((toast) => (
            <motion.div
              key={toast.id}
              initial={{ opacity: 0, y: 20, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 10, scale: 0.95 }}
              transition={{ type: 'spring', damping: 25, stiffness: 400 }}
              className="px-4 py-3 glass-card rounded-xl border border-[var(--glass-border)] shadow-xl flex items-center gap-3 pointer-events-auto"
            >
              <div className={`w-7 h-7 rounded-full ${getBgColor(toast.type)} flex items-center justify-center`}>
                {getIcon(toast.type)}
              </div>
              <p className="text-sm font-medium text-[var(--text-primary)] whitespace-nowrap">
                {toast.message}
              </p>
              {toast.action && (
                <button
                  onClick={() => {
                    toast.action?.onClick()
                    removeToast(toast.id)
                  }}
                  className="ml-2 px-3 py-1.5 text-xs font-medium rounded-lg bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90 transition-colors flex items-center gap-1"
                >
                  <RefreshCw className="w-3 h-3" />
                  {toast.action.label}
                </button>
              )}
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </ToastContext.Provider>
  )
}

function ToastProviderInner({ 
  children, 
  showToast,
  isRestarting,
  setIsRestarting
}: { 
  children: ReactNode
  showToast: (message: string, type?: ToastType, action?: ToastAction) => void
  isRestarting: boolean
  setIsRestarting: (v: boolean) => void
}) {
  const { state: vpnState, setNeedsRestart } = useConnectionStore()

  const restartVpn = useCallback(async () => {
    if (isRestarting) return
    setIsRestarting(true)
    try {
      const result = await window.api.singbox.restart()
      if (result.success) {
        setNeedsRestart(false)
        showToast('VPN 已重启', 'success')
      } else {
        showToast(`重启失败: ${result.error}`, 'error')
      }
    } catch {
      showToast('重启失败', 'error')
    } finally {
      setIsRestarting(false)
    }
  }, [isRestarting, setIsRestarting, setNeedsRestart, showToast])

  const showRestartToast = useCallback((message: string) => {
    if (vpnState === 'connected') {
      showToast(message, 'info', {
        label: '重启',
        onClick: restartVpn
      })
    } else {
      showToast(message, 'success')
    }
  }, [vpnState, showToast, restartVpn])

  return (
    <ToastContext.Provider value={{ showToast, showRestartToast, success: (m) => showToast(m, 'success'), error: (m) => showToast(m, 'error'), warning: (m) => showToast(m, 'warning'), info: (m) => showToast(m, 'info') }}>
      {children}
    </ToastContext.Provider>
  )
}
