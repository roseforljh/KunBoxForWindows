import { useState } from 'react'
import { motion } from 'framer-motion'
import { ChevronLeft, Loader2, Plus } from 'lucide-react'
import {
  buildManualNodeLink,
  createManualNodeDraft,
  MANUAL_NODE_PROTOCOLS,
  type ManualNodeDefinition,
  type ManualNodeDraft,
  type ManualNodeProtocol,
  type ManualTlsMode,
  type ManualTransport,
} from './manual-node'
import { AppSelect } from './Select'

interface ManualNodeEditorProps {
  disabled?: boolean
  onCancel: () => void
  onSave: (node: ManualNodeDefinition) => void
}

const INPUT_CLASS = 'w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50'

function Field({ label, value, onChange, placeholder, type = 'text', disabled, autoFocus }: {
  label: string
  value: string
  onChange: (value: string) => void
  placeholder?: string
  type?: 'text' | 'password' | 'number'
  disabled?: boolean
  autoFocus?: boolean
}) {
  return (
    <label className="space-y-1.5">
      <span className="block text-xs font-medium text-[var(--text-muted)]">{label}</span>
      <input
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        autoFocus={autoFocus}
        className={INPUT_CLASS}
      />
    </label>
  )
}

function SelectField({ label, value, onChange, options, disabled }: {
  label: string
  value: string
  onChange: (value: string) => void
  options: Array<{ value: string; label: string }>
  disabled?: boolean
}) {
  return (
    <label className="space-y-1.5">
      <span className="block text-xs font-medium text-[var(--text-muted)]">{label}</span>
      <AppSelect
        value={value}
        options={options}
        onValueChange={onChange}
        disabled={disabled}
        ariaLabel={label}
      />
    </label>
  )
}

function ToggleField({ label, description, checked, onChange, disabled }: {
  label: string
  description: string
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
}) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl border border-[var(--border-secondary)] bg-[var(--bg-elevated)]/50 px-3 py-2.5">
      <div>
        <p className="text-sm font-medium text-[var(--text-primary)]">{label}</p>
        <p className="text-xs text-[var(--text-faint)]">{description}</p>
      </div>
      <button
        type="button"
        aria-pressed={checked}
        onClick={() => onChange(!checked)}
        disabled={disabled}
        className={`w-11 h-6 rounded-full transition-colors ${checked ? 'bg-[var(--accent-primary)]' : 'bg-[var(--border-secondary)]'} disabled:opacity-50`}
      >
        <span className={`block w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${checked ? 'translate-x-5' : 'translate-x-0.5'}`} />
      </button>
    </div>
  )
}

export function ManualNodeEditor({ disabled, onCancel, onSave }: ManualNodeEditorProps) {
  const [protocol, setProtocol] = useState<ManualNodeProtocol | null>(null)
  const [draft, setDraft] = useState<ManualNodeDraft | null>(null)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)

  const selectProtocol = (nextProtocol: ManualNodeProtocol) => {
    setProtocol(nextProtocol)
    setDraft(createManualNodeDraft(nextProtocol))
    setError('')
  }

  const update = <K extends keyof ManualNodeDraft>(key: K, value: ManualNodeDraft[K]) => {
    setDraft((current) => current ? { ...current, [key]: value } : current)
    setError('')
  }

  const save = () => {
    if (!draft || !protocol) return
    setSaving(true)
    try {
      onSave({ protocol, tag: draft.name.trim(), link: buildManualNodeLink(draft) })
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '节点参数无效')
    } finally {
      setSaving(false)
    }
  }

  if (!protocol || !draft) {
    return (
      <motion.div initial={{ opacity: 0, x: 12 }} animate={{ opacity: 1, x: 0 }} className="space-y-4">
        <div className="flex items-center justify-between gap-3">
          <button type="button" onClick={onCancel} className="flex items-center gap-1 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)]">
            <ChevronLeft className="w-4 h-4" /> 返回节点列表
          </button>
          <span className="text-xs text-[var(--text-faint)]">选择要配置的协议</span>
        </div>
        <div className="grid grid-cols-2 sm:grid-cols-3 gap-2.5">
          {MANUAL_NODE_PROTOCOLS.map((item) => (
            <button
              type="button"
              key={item.id}
              onClick={() => selectProtocol(item.id)}
              disabled={disabled}
              className="group min-h-[92px] rounded-xl border border-[var(--border-secondary)] bg-[var(--bg-elevated)]/45 p-3 text-left hover:border-[var(--accent-primary)] hover:bg-[var(--accent-muted)] transition-colors disabled:opacity-50"
            >
              <span className="inline-flex min-w-8 h-6 px-1.5 items-center justify-center rounded-md bg-[var(--accent-muted)] font-mono text-[11px] font-bold text-[var(--accent-primary)]">
                {item.code}
              </span>
              <span className="block mt-2 text-sm font-semibold text-[var(--text-primary)]">{item.label}</span>
              <span className="block mt-0.5 text-[11px] text-[var(--text-faint)]">{item.description}</span>
            </button>
          ))}
        </div>
      </motion.div>
    )
  }

  const option = MANUAL_NODE_PROTOCOLS.find((item) => item.id === protocol)!
  const supportsTransport = protocol === 'vmess' || protocol === 'vless' || protocol === 'trojan'
  const tlsRequired = ['trojan', 'hysteria2', 'hysteria', 'tuic', 'anytls', 'naive'].includes(protocol)
  const showTls = tlsRequired || protocol === 'http' || protocol === 'vmess' || protocol === 'vless'
  const transportOptions = protocol === 'vless'
    ? [
        { value: 'tcp', label: 'TCP' },
        { value: 'ws', label: 'WebSocket' },
        { value: 'grpc', label: 'gRPC' },
        { value: 'xhttp', label: 'XHTTP' },
      ]
    : [
        { value: 'tcp', label: 'TCP' },
        { value: 'ws', label: 'WebSocket' },
        { value: 'grpc', label: 'gRPC' },
      ]

  return (
    <motion.div initial={{ opacity: 0, x: 12 }} animate={{ opacity: 1, x: 0 }} className="space-y-5">
      <div className="flex items-center justify-between gap-3">
        <button type="button" onClick={() => { setProtocol(null); setDraft(null); setError('') }} className="flex items-center gap-1 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)]">
          <ChevronLeft className="w-4 h-4" /> 重新选择协议
        </button>
        <div className="flex items-center gap-2">
          <span className="inline-flex h-6 px-2 items-center rounded-md bg-[var(--accent-muted)] font-mono text-[11px] font-bold text-[var(--accent-primary)]">{option.code}</span>
          <span className="text-sm font-semibold text-[var(--text-primary)]">{option.label}</span>
        </div>
      </div>

      <section className="space-y-3">
        <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">基本信息</p>
        <Field label="节点名称" value={draft.name} onChange={(value) => update('name', value)} placeholder={`${option.label} 节点`} disabled={disabled} autoFocus />
        <div className="grid grid-cols-[minmax(0,1fr)_120px] gap-3">
          <Field label="服务器地址" value={draft.server} onChange={(value) => update('server', value)} placeholder="example.com 或 IP" disabled={disabled} />
          <Field label="端口" type="number" value={draft.port} onChange={(value) => update('port', value)} disabled={disabled} />
        </div>
      </section>

      {(protocol === 'socks5' || protocol === 'http' || protocol === 'naive') && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">身份验证</p>
          <div className="grid grid-cols-2 gap-3">
            <Field label={protocol === 'naive' ? '用户名' : '用户名（可选）'} value={draft.username} onChange={(value) => update('username', value)} disabled={disabled} />
            <Field label={protocol === 'naive' ? '密码' : '密码（可选）'} type="password" value={draft.password} onChange={(value) => update('password', value)} disabled={disabled} />
          </div>
        </section>
      )}

      {protocol === 'shadowsocks' && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">加密参数</p>
          <div className="grid grid-cols-2 gap-3">
            <SelectField label="加密方式" value={draft.method} onChange={(value) => update('method', value)} disabled={disabled} options={[
              { value: 'aes-128-gcm', label: 'AES-128-GCM' },
              { value: 'aes-256-gcm', label: 'AES-256-GCM' },
              { value: 'chacha20-ietf-poly1305', label: 'ChaCha20-IETF-Poly1305' },
              { value: '2022-blake3-aes-128-gcm', label: '2022 BLAKE3 AES-128-GCM' },
              { value: '2022-blake3-aes-256-gcm', label: '2022 BLAKE3 AES-256-GCM' },
            ]} />
            <Field label="密码" type="password" value={draft.password} onChange={(value) => update('password', value)} disabled={disabled} />
          </div>
        </section>
      )}

      {(protocol === 'vmess' || protocol === 'vless' || protocol === 'tuic') && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">用户凭据</p>
          <Field label="UUID" value={draft.uuid} onChange={(value) => update('uuid', value)} placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" disabled={disabled} />
          {protocol === 'vmess' && (
            <div className="grid grid-cols-2 gap-3">
              <SelectField label="加密方式" value={draft.security} onChange={(value) => update('security', value)} disabled={disabled} options={[
                { value: 'auto', label: 'Auto' },
                { value: 'aes-128-gcm', label: 'AES-128-GCM' },
                { value: 'chacha20-poly1305', label: 'ChaCha20-Poly1305' },
                { value: 'none', label: 'None' },
              ]} />
              <Field label="Alter ID" type="number" value={draft.alterId} onChange={(value) => update('alterId', value)} disabled={disabled} />
            </div>
          )}
          {protocol === 'vless' && <Field label="Flow（可选）" value={draft.flow} onChange={(value) => update('flow', value)} placeholder="xtls-rprx-vision" disabled={disabled} />}
          {protocol === 'tuic' && <Field label="密码" type="password" value={draft.password} onChange={(value) => update('password', value)} disabled={disabled} />}
        </section>
      )}

      {(protocol === 'trojan' || protocol === 'hysteria2' || protocol === 'anytls') && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">身份验证</p>
          <Field label="密码" type="password" value={draft.password} onChange={(value) => update('password', value)} disabled={disabled} />
        </section>
      )}

      {protocol === 'hysteria' && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">连接参数</p>
          <Field label="认证字符串（可选）" type="password" value={draft.auth} onChange={(value) => update('auth', value)} disabled={disabled} />
          <div className="grid grid-cols-2 gap-3">
            <Field label="上传带宽 Mbps" type="number" value={draft.upMbps} onChange={(value) => update('upMbps', value)} disabled={disabled} />
            <Field label="下载带宽 Mbps" type="number" value={draft.downMbps} onChange={(value) => update('downMbps', value)} disabled={disabled} />
          </div>
        </section>
      )}

      {protocol === 'hysteria2' && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">混淆</p>
          <SelectField label="混淆方式" value={draft.obfs} onChange={(value) => update('obfs', value)} disabled={disabled} options={[
            { value: 'none', label: '不使用' },
            { value: 'salamander', label: 'Salamander' },
          ]} />
          {draft.obfs !== 'none' && <Field label="混淆密码" type="password" value={draft.obfsPassword} onChange={(value) => update('obfsPassword', value)} disabled={disabled} />}
        </section>
      )}

      {protocol === 'tuic' && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">QUIC 参数</p>
          <div className="grid grid-cols-2 gap-3">
            <SelectField label="拥塞控制" value={draft.congestionControl} onChange={(value) => update('congestionControl', value)} disabled={disabled} options={[
              { value: 'bbr', label: 'BBR' },
              { value: 'cubic', label: 'Cubic' },
              { value: 'new_reno', label: 'New Reno' },
            ]} />
            <SelectField label="UDP 中继" value={draft.udpRelayMode} onChange={(value) => update('udpRelayMode', value)} disabled={disabled} options={[
              { value: 'native', label: 'Native' },
              { value: 'quic', label: 'QUIC' },
            ]} />
          </div>
          <Field label="ALPN（可选）" value={draft.alpn} onChange={(value) => update('alpn', value)} placeholder="h3" disabled={disabled} />
        </section>
      )}

      {supportsTransport && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">传输方式</p>
          <SelectField label="传输协议" value={draft.transport} onChange={(value) => update('transport', value as ManualTransport)} disabled={disabled} options={transportOptions} />
          {(draft.transport === 'ws' || draft.transport === 'xhttp') && (
            <div className="grid grid-cols-2 gap-3">
              <Field label="路径" value={draft.path} onChange={(value) => update('path', value)} placeholder="/" disabled={disabled} />
              <Field label="Host（可选）" value={draft.host} onChange={(value) => update('host', value)} disabled={disabled} />
            </div>
          )}
          {draft.transport === 'grpc' && <Field label="Service Name（可选）" value={draft.serviceName} onChange={(value) => update('serviceName', value)} disabled={disabled} />}
        </section>
      )}

      {showTls && (
        <section className="space-y-3">
          <p className="text-xs font-semibold tracking-wide text-[var(--text-faint)]">TLS</p>
          {!tlsRequired && (
            <SelectField
              label="安全模式"
              value={draft.tlsMode}
              onChange={(value) => update('tlsMode', value as ManualTlsMode)}
              disabled={disabled}
              options={protocol === 'vless'
                ? [{ value: 'none', label: '不使用' }, { value: 'tls', label: 'TLS' }, { value: 'reality', label: 'Reality' }]
                : [{ value: 'none', label: '不使用' }, { value: 'tls', label: 'TLS' }]}
            />
          )}
          {(tlsRequired || draft.tlsMode !== 'none') && (
            <>
              <Field label="服务器名称 SNI（留空使用服务器地址）" value={draft.serverName} onChange={(value) => update('serverName', value)} disabled={disabled} />
              {draft.tlsMode === 'reality' && (
                <div className="grid grid-cols-2 gap-3">
                  <Field label="Reality 公钥" value={draft.publicKey} onChange={(value) => update('publicKey', value)} disabled={disabled} />
                  <Field label="Short ID（可选）" value={draft.shortId} onChange={(value) => update('shortId', value)} disabled={disabled} />
                </div>
              )}
              <ToggleField label="跳过证书验证" description="仅用于自签名证书" checked={draft.allowInsecure} onChange={(value) => update('allowInsecure', value)} disabled={disabled} />
            </>
          )}
        </section>
      )}

      {error && (
        <div className="px-3 py-2 bg-[var(--status-error)]/10 border border-[var(--status-error)]/30 rounded-lg">
          <p className="text-sm text-[var(--status-error)]">{error}</p>
        </div>
      )}

      <div className="flex items-center justify-end gap-2 pt-2 border-t border-[var(--border-secondary)]">
        <button type="button" onClick={onCancel} disabled={disabled || saving} className="px-4 py-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)] disabled:opacity-50">
          取消
        </button>
        <button type="button" onClick={save} disabled={disabled || saving} className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-primary)] text-white rounded-lg text-sm font-medium hover:opacity-90 disabled:opacity-50">
          {saving ? <Loader2 className="w-4 h-4 animate-spin" /> : <Plus className="w-4 h-4" />}
          添加到订阅
        </button>
      </div>
    </motion.div>
  )
}
