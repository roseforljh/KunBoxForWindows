import { useState } from 'react'
import { createPortal } from 'react-dom'
import { motion, AnimatePresence } from 'framer-motion'
import { X, Link, FileText, Clipboard, Loader2, ChevronDown } from 'lucide-react'

type ImportType = 'url' | 'clipboard' | 'file'

interface ProfileSettings {
  autoUpdateInterval: number
  dnsPreResolve: boolean
  dnsServer: string | null
}

interface AddProfileModalProps {
  isOpen: boolean
  onClose: () => void
  onImportUrl: (name: string, url: string, settings: ProfileSettings) => Promise<void>
  onImportContent: (name: string, content: string, settings: ProfileSettings) => Promise<void>
}

const IMPORT_TYPES: { id: ImportType; label: string; icon: typeof Link; description: string }[] = [
  { id: 'url', label: '订阅链接', icon: Link, description: '从 URL 导入订阅' },
  { id: 'clipboard', label: '剪贴板', icon: Clipboard, description: '从剪贴板内容导入' },
  { id: 'file', label: '本地文件', icon: FileText, description: '从 JSON/YAML 文件导入' }
]

const DNS_SERVERS = [
  { value: 'https://cloudflare-dns.com/dns-query', label: 'Cloudflare DNS' },
  { value: 'https://dns.google/dns-query', label: 'Google DNS' },
  { value: 'https://dns.alidns.com/dns-query', label: '阿里云 DNS' }
]

export function AddProfileModal({ isOpen, onClose, onImportUrl, onImportContent }: AddProfileModalProps) {
  const [importType, setImportType] = useState<ImportType>('url')
  const [name, setName] = useState('')
  const [url, setUrl] = useState('')
  const [content, setContent] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const [autoUpdateEnabled, setAutoUpdateEnabled] = useState(false)
  const [autoUpdateMinutes, setAutoUpdateMinutes] = useState('60')
  const [dnsPreResolve, setDnsPreResolve] = useState(false)
  const [dnsServer, setDnsServer] = useState(DNS_SERVERS[0].value)
  const [dnsDropdownOpen, setDnsDropdownOpen] = useState(false)

  const handleClose = () => {
    if (loading) return
    setName('')
    setUrl('')
    setContent('')
    setError('')
    setImportType('url')
    setAutoUpdateEnabled(false)
    setAutoUpdateMinutes('60')
    setDnsPreResolve(false)
    setDnsServer(DNS_SERVERS[0].value)
    onClose()
  }

  const handlePaste = async () => {
    try {
      const text = await navigator.clipboard.readText()
      setContent(text)
    } catch {
      setError('无法读取剪贴板')
    }
  }

  const handleFileSelect = async () => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = '.json,.yaml,.yml,.txt'
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0]
      if (!file) return

      try {
        const text = await file.text()
        setContent(text)
        if (!name) {
          setName(file.name.replace(/\.(json|yaml|yml|txt)$/i, ''))
        }
      } catch {
        setError('无法读取文件')
      }
    }
    input.click()
  }

  const handleImport = async () => {
    setError('')
    setLoading(true)

    const settings: ProfileSettings = {
      autoUpdateInterval: autoUpdateEnabled ? Math.max(15, parseInt(autoUpdateMinutes) || 60) : 0,
      dnsPreResolve,
      dnsServer: dnsPreResolve ? dnsServer : null
    }

    try {
      if (importType === 'url') {
        if (!url.trim()) {
          setError('请输入订阅链接')
          setLoading(false)
          return
        }
        if (autoUpdateEnabled && (parseInt(autoUpdateMinutes) || 0) < 15) {
          setError('自动更新间隔最少 15 分钟')
          setLoading(false)
          return
        }
        await onImportUrl(name.trim(), url.trim(), settings)
      } else {
        if (!content.trim()) {
          setError('内容不能为空')
          setLoading(false)
          return
        }
        await onImportContent(name.trim() || 'Imported', content.trim(), settings)
      }
      handleClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : '导入失败')
    } finally {
      setLoading(false)
    }
  }

  const canImport = importType === 'url' ? url.trim() : content.trim()

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm"
            onClick={handleClose}
          />
          <motion.div
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ type: 'spring', damping: 25, stiffness: 300 }}
            className="relative w-[480px] glass-card rounded-2xl border border-[var(--glass-border)] shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between p-5 border-b border-[var(--border-secondary)]">
              <h3 className="text-lg font-bold text-[var(--text-primary)]">添加订阅</h3>
              <button
                onClick={handleClose}
                disabled={loading}
                className="w-8 h-8 flex items-center justify-center rounded-lg hover:bg-[var(--bg-elevated)] transition-colors disabled:opacity-50"
              >
                <X className="w-4 h-4 text-[var(--text-muted)]" />
              </button>
            </div>

            <div className="p-5 space-y-5">
              <div className="space-y-2">
                <label className="text-sm font-medium text-[var(--text-muted)]">导入方式</label>
                <div className="grid grid-cols-3 gap-2">
                  {IMPORT_TYPES.map((type) => {
                    const Icon = type.icon
                    return (
                      <button
                        key={type.id}
                        onClick={() => setImportType(type.id)}
                        disabled={loading}
                        className={`p-3 rounded-xl border transition-all ${
                          importType === type.id
                            ? 'border-[var(--accent-primary)] bg-[var(--accent-muted)]'
                            : 'border-[var(--border-secondary)] hover:border-[var(--text-faint)]'
                        } disabled:opacity-50`}
                      >
                        <Icon className={`w-5 h-5 mx-auto mb-1 ${
                          importType === type.id ? 'text-[var(--accent-primary)]' : 'text-[var(--text-muted)]'
                        }`} />
                        <p className={`text-sm font-semibold ${
                          importType === type.id ? 'text-[var(--accent-primary)]' : 'text-[var(--text-primary)]'
                        }`}>
                          {type.label}
                        </p>
                      </button>
                    )
                  })}
                </div>
                <p className="text-xs text-[var(--text-faint)]">
                  {IMPORT_TYPES.find(t => t.id === importType)?.description}
                </p>
              </div>

              <div className="space-y-2">
                <label className="text-sm font-medium text-[var(--text-muted)]">
                  名称 <span className="text-[var(--text-faint)]">(可选)</span>
                </label>
                <input
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="自动从链接提取"
                  disabled={loading}
                  className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                />
              </div>

              <AnimatePresence mode="wait">
                {importType === 'url' ? (
                  <motion.div
                    key="url-input"
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.2, ease: 'easeInOut' }}
                    className="overflow-hidden"
                  >
                    <div className="space-y-2">
                      <label className="text-sm font-medium text-[var(--text-muted)]">订阅链接</label>
                      <input
                        type="text"
                        value={url}
                        onChange={(e) => setUrl(e.target.value)}
                        placeholder="https://..."
                        disabled={loading}
                        className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                      />
                    </div>
                  </motion.div>
                ) : (
                  <motion.div
                    key="content-input"
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.2, ease: 'easeInOut' }}
                    className="overflow-hidden"
                  >
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <label className="text-sm font-medium text-[var(--text-muted)]">内容</label>
                        <div className="flex gap-2">
                          {importType === 'clipboard' && (
                            <button
                              onClick={handlePaste}
                              disabled={loading}
                              className="text-xs text-[var(--accent-primary)] hover:underline disabled:opacity-50"
                            >
                              粘贴
                            </button>
                          )}
                          {importType === 'file' && (
                            <button
                              onClick={handleFileSelect}
                              disabled={loading}
                              className="text-xs text-[var(--accent-primary)] hover:underline disabled:opacity-50"
                            >
                              选择文件
                            </button>
                          )}
                        </div>
                      </div>
                      <textarea
                        value={content}
                        onChange={(e) => setContent(e.target.value)}
                        placeholder={importType === 'clipboard' ? '点击"粘贴"或手动粘贴内容...' : '选择文件或粘贴内容...'}
                        disabled={loading}
                        rows={6}
                        className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] resize-none font-mono disabled:opacity-50"
                      />
                      <p className="text-xs text-[var(--text-faint)]">
                        支持 YAML、JSON、Base64 编码或节点链接格式
                      </p>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>

              <div className="pt-2 border-t border-[var(--border-secondary)]">
                <div className="flex items-center justify-between py-2">
                  <div>
                    <p className="text-sm font-medium text-[var(--text-primary)]">自动更新</p>
                    <p className="text-xs text-[var(--text-faint)]">定时更新订阅内容</p>
                  </div>
                  <button
                    onClick={() => setAutoUpdateEnabled(!autoUpdateEnabled)}
                    disabled={loading}
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
                          disabled={loading}
                          className="mt-1 w-full px-3 py-2 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                        />
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>

                <div className="flex items-center justify-between py-2 mt-2">
                  <div>
                    <p className="text-sm font-medium text-[var(--text-primary)]">DNS 预解析</p>
                    <p className="text-xs text-[var(--text-faint)]">启动前预解析节点域名，加快连接</p>
                  </div>
                  <button
                    onClick={() => setDnsPreResolve(!dnsPreResolve)}
                    disabled={loading}
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
                          disabled={loading}
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

            <div className="flex items-center justify-end gap-2 p-5 border-t border-[var(--border-secondary)]">
              <button
                onClick={handleClose}
                disabled={loading}
                className="px-4 py-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors disabled:opacity-50"
              >
                取消
              </button>
              <button
                onClick={handleImport}
                disabled={loading || !canImport}
                className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-primary)] text-white rounded-lg text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {loading && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>{loading ? '导入中...' : '导入'}</span>
              </button>
            </div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>,
    document.body
  )
}
