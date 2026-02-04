import { motion, AnimatePresence } from 'framer-motion'
import { createPortal } from 'react-dom'
import { Trash2, AlertTriangle, Info, X, Loader2 } from 'lucide-react'
import { ModalButton } from './Modal'

interface ConfirmModalProps {
  isOpen: boolean
  onClose: () => void
  onConfirm: () => void
  title: string
  description: string
  confirmText?: string
  cancelText?: string
  variant?: 'danger' | 'warning' | 'info'
  isLoading?: boolean
}

export function ConfirmModal({
  isOpen,
  onClose,
  onConfirm,
  title,
  description,
  confirmText = '确认',
  cancelText = '取消',
  variant = 'danger',
  isLoading = false
}: ConfirmModalProps) {
  const icons = {
    danger: <Trash2 className="w-8 h-8" />,
    warning: <AlertTriangle className="w-8 h-8" />,
    info: <Info className="w-8 h-8" />
  }

  const colors = {
    danger: 'text-[var(--status-error)] bg-[var(--status-error)]/10',
    warning: 'text-yellow-500 bg-yellow-500/10',
    info: 'text-[var(--accent-primary)] bg-[var(--accent-primary)]/10'
  }

  const accentColors = {
    danger: 'var(--status-error)',
    warning: '#eab308',
    info: 'var(--accent-primary)'
  }

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 bg-black/60 backdrop-blur-xl"
            style={{ WebkitBackdropFilter: 'blur(24px)' }}
            onClick={onClose}
          />

          <motion.div
            initial={{ scale: 0.9, opacity: 0, y: 20 }}
            animate={{ scale: 1, opacity: 1, y: 0 }}
            exit={{ scale: 0.9, opacity: 0, y: 20 }}
            transition={{ type: 'spring', duration: 0.5, bounce: 0.3 }}
            className="relative w-full max-w-[420px] rounded-3xl border border-[var(--glass-border)] flex flex-col shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="absolute inset-0 bg-gradient-to-br from-[var(--bg-primary)] via-[var(--bg-secondary)] to-[var(--bg-primary)] rounded-3xl" />
            <div className="absolute inset-0 bg-[var(--bg-primary)]/80 backdrop-blur-3xl rounded-3xl" />

            <motion.div
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 0.12, scale: 1 }}
              transition={{ delay: 0.1, duration: 0.8 }}
              className="absolute -top-[25%] -right-[15%] w-72 h-72 blur-[100px] rounded-full pointer-events-none"
              style={{ backgroundColor: accentColors[variant] }}
            />
            <motion.div
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 0.1, scale: 1 }}
              transition={{ delay: 0.2, duration: 0.8 }}
              className="absolute -bottom-[25%] -left-[15%] w-72 h-72 blur-[100px] rounded-full pointer-events-none"
              style={{ backgroundColor: accentColors[variant] }}
            />

            <div
              className="absolute top-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-current to-transparent opacity-30"
              style={{ color: accentColors[variant] }}
            />

            <div className="relative z-10 p-8 flex flex-col items-center text-center">
              <div className={`w-16 h-16 rounded-2xl flex items-center justify-center mb-6 ${colors[variant]}`}>
                {icons[variant]}
              </div>

              <h3 className="text-2xl font-bold text-[var(--text-primary)] mb-3 tracking-tight">
                {title}
              </h3>

              <p className="text-sm text-[var(--text-muted)] leading-relaxed px-2 font-medium">
                {description}
              </p>
            </div>

            <div className="relative z-10 px-8 py-6 flex gap-3 bg-[var(--bg-secondary)]/30 border-t border-[var(--glass-border)]">
              <ModalButton
                variant="secondary"
                onClick={onClose}
                disabled={isLoading}
                className="flex-1 py-3"
              >
                {cancelText}
              </ModalButton>
              <ModalButton
                variant={variant === 'danger' ? 'danger' : 'primary'}
                onClick={onConfirm}
                disabled={isLoading}
                className="flex-[1.5] py-3"
              >
                {isLoading ? <Loader2 className="w-4 h-4 animate-spin" /> : confirmText}
              </ModalButton>
            </div>

            <button
              onClick={onClose}
              className="absolute top-4 right-4 p-2 rounded-xl text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-white/5 transition-all active:scale-95 z-20"
            >
              <X className="w-5 h-5" />
            </button>
          </motion.div>
        </div>
      )}
    </AnimatePresence>,
    document.body
  )
}
