import { useState, useEffect } from 'react'
import { Modal, ModalButton } from './Modal'
import { Loader2, ChevronDown } from 'lucide-react'
import { motion, AnimatePresence } from 'framer-motion'

interface ProfileSettings {
  autoUpdateInterval: number
  dnsPreResolve: boolean
  dnsServer: string | null
}

interface EditProfileModalProps {
  isOpen: boolean
  onClose: () => void
  onSave: (name: string, url: string, settings: ProfileSettings) => void
  initialName: string
  initialUrl: string
  initialAutoUpdateInterval?: number
  initialDnsPreResolve?: boolean
  initialDnsServer?: string | null
  isLoading?: boolean
}

const DNS_SERVERS = [
  { value: 'https://cloudflare-dns.com/dns-query', label: 'Cloudflare DNS' },
  { value: 'https://dns.google/dns-query', label: 'Google DNS' },
  { value: 'https://dns.alidns.com/dns-query', label: '阿里云 DNS' }
]

export function EditProfileModal({
  isOpen,
  onClose,
  onSave,
  initialName,
  initialUrl,
  initialAutoUpdateInterval = 0,
  initialDnsPreResolve = false,
  initialDnsServer = null,
  isLoading = false
}: EditProfileModalProps) {
  const [name, setName] = useState(initialName)
  const [url, setUrl] = useState(initialUrl)
  const [autoUpdateEnabled, setAutoUpdateEnabled] = useState(initialAutoUpdateInterval > 0)
  const [autoUpdateMinutes, setAutoUpdateMinutes] = useState(initialAutoUpdateInterval > 0 ? initialAutoUpdateInterval.toString() : '60')
  const [dnsPreResolve, setDnsPreResolve] = useState(initialDnsPreResolve)
  const [dnsServer, setDnsServer] = useState(initialDnsServer || DNS_SERVERS[0].value)
  const [dnsDropdownOpen, setDnsDropdownOpen] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (isOpen) {
      setName(initialName)
      setUrl(initialUrl)
      setAutoUpdateEnabled(initialAutoUpdateInterval > 0)
      setAutoUpdateMinutes(initialAutoUpdateInterval > 0 ? initialAutoUpdateInterval.toString() : '60')
      setDnsPreResolve(initialDnsPreResolve)
      setDnsServer(initialDnsServer || DNS_SERVERS[0].value)
      setError('')
    }
  }, [isOpen, initialName, initialUrl, initialAutoUpdateInterval, initialDnsPreResolve, initialDnsServer])

  const handleSave = () => {
    if (!name.trim()) {
      setError('名称不能为空')
      return
    }
    if (!url.trim()) {
      setError('订阅地址不能为空')
      return
    }
    if (autoUpdateEnabled && (parseInt(autoUpdateMinutes) || 0) < 15) {
      setError('自动更新间隔最少 15 分钟')
      return
    }

    const settings: ProfileSettings = {
      autoUpdateInterval: autoUpdateEnabled ? Math.max(15, parseInt(autoUpdateMinutes) || 60) : 0,
      dnsPreResolve,
      dnsServer: dnsPreResolve ? dnsServer : null
    }

    onSave(name.trim(), url.trim(), settings)
  }

  const isValid = name.trim().length > 0 && url.trim().length > 0

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="编辑订阅"
      maxWidth="max-w-md"
      footer={
        <>
          <ModalButton variant="secondary" onClick={onClose} disabled={isLoading}>
            取消
          </ModalButton>
          <ModalButton
            variant="primary"
            onClick={handleSave}
            disabled={isLoading || !isValid}
          >
            {isLoading ? <Loader2 className="w-4 h-4 animate-spin" /> : '保存'}
          </ModalButton>
        </>
      }
    >
      <div className="space-y-4">
        <div>
          <label className="block text-sm font-medium text-[var(--text-secondary)] mb-2">
            订阅名称
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="输入订阅名称..."
            className="glass-input w-full rounded-xl"
            autoFocus
          />
        </div>
        <div>
          <label className="block text-sm font-medium text-[var(--text-secondary)] mb-2">
            订阅地址
          </label>
          <input
            type="text"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="输入订阅地址..."
            className="glass-input w-full rounded-xl"
          />
        </div>

        <div className="pt-3 border-t border-[var(--border-secondary)]">
          <div className="flex items-center justify-between py-2">
            <div>
              <p className="text-sm font-medium text-[var(--text-primary)]">自动更新</p>
              <p className="text-xs text-[var(--text-faint)]">定时更新订阅内容</p>
            </div>
            <button
              onClick={() => setAutoUpdateEnabled(!autoUpdateEnabled)}
              disabled={isLoading}
              className={`w-11 h-6 rounded-full transition-colors ${
                autoUpdateEnabled ? 'bg-[var(--accent-primary)]' : 'bg-[var(--bg-elevated)]'
              } disabled:opacity-50`}
            >
              <div className={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                autoUpdateEnabled ? 'translate-x-5' : 'translate-x-0.5'
              }`} />
            </button>
          </div>

          <AnimatePresence>
            {autoUpdateEnabled && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: 'auto', opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.2 }}
                className="overflow-hidden"
              >
                <div className="pt-2 pb-1">
                  <label className="text-xs text-[var(--text-muted)]">更新间隔（分钟，最少 15）</label>
                  <input
                    type="number"
                    value={autoUpdateMinutes}
                    onChange={(e) => setAutoUpdateMinutes(e.target.value.replace(/\D/g, ''))}
                    min={15}
                    disabled={isLoading}
                    className="mt-1 w-full px-3 py-2 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                  />
                </div>
              </motion.div>
            )}
          </AnimatePresence>

          <div className="flex items-center justify-between py-2 mt-2">
            <div>
              <p className="text-sm font-medium text-[var(--text-primary)]">DNS 预解析</p>
              <p className="text-xs text-[var(--text-faint)]">启动前预解析节点域名</p>
            </div>
            <button
              onClick={() => setDnsPreResolve(!dnsPreResolve)}
              disabled={isLoading}
              className={`w-11 h-6 rounded-full transition-colors ${
                dnsPreResolve ? 'bg-[var(--accent-primary)]' : 'bg-[var(--bg-elevated)]'
              } disabled:opacity-50`}
            >
              <div className={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                dnsPreResolve ? 'translate-x-5' : 'translate-x-0.5'
              }`} />
            </button>
          </div>

          <AnimatePresence>
            {dnsPreResolve && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: 'auto', opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.2 }}
                className="overflow-hidden"
              >
                <div className="pt-2 pb-1 relative">
                  <label className="text-xs text-[var(--text-muted)]">DNS 服务器</label>
                  <button
                    onClick={() => setDnsDropdownOpen(!dnsDropdownOpen)}
                    disabled={isLoading}
                    className="mt-1 w-full px-3 py-2 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] flex items-center justify-between disabled:opacity-50"
                  >
                    <span>{DNS_SERVERS.find(s => s.value === dnsServer)?.label || dnsServer}</span>
                    <ChevronDown className={`w-4 h-4 text-[var(--text-muted)] transition-transform ${dnsDropdownOpen ? 'rotate-180' : ''}`} />
                  </button>
                  {dnsDropdownOpen && (
                    <div className="absolute left-0 right-0 top-full mt-1 bg-[var(--glass-bg)] border border-[var(--glass-border)] rounded-lg shadow-xl z-10 overflow-hidden">
                      {DNS_SERVERS.map((server) => (
                        <button
                          key={server.value}
                          onClick={() => {
                            setDnsServer(server.value)
                            setDnsDropdownOpen(false)
                          }}
                          className={`w-full px-3 py-2.5 text-left text-sm hover:bg-[var(--bg-elevated)] transition-colors ${
                            dnsServer === server.value ? 'text-[var(--accent-primary)] bg-[var(--accent-muted)]' : 'text-[var(--text-primary)]'
                          }`}
                        >
                          {server.label}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {error && (
          <div className="px-3 py-2 bg-[var(--status-error)]/10 border border-[var(--status-error)]/30 rounded-lg">
            <p className="text-sm text-[var(--status-error)]">{error}</p>
          </div>
        )}
      </div>
    </Modal>
  )
}
