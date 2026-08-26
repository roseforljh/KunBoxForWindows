import { useEffect, useMemo, useState } from 'react'
import { Check, Copy, Loader2, Save } from 'lucide-react'
import type {
  NodeWithProfile,
  OutboundMultiplex,
  OutboundTls,
  OutboundTransport,
  SingBoxOutbound,
} from '@shared/types'
import { Modal, ModalButton } from './Modal'
import { useManagedTimeouts } from '../../lib/useManagedTimeouts'
import {
  applyNodePolicies,
  makeNodeReference,
  nodePolicyState,
  toEditableNode,
  validateNodeForSave,
} from './node-editor'
import { AppSelect } from './Select'

interface NodeDetailModalProps {
  isOpen: boolean
  onClose: () => void
  node: SingBoxOutbound | null
  profileId: string | null
  onSave: (originalTag: string, node: SingBoxOutbound) => Promise<void>
  onExport?: (tag: string) => Promise<void>
}

const PROTOCOL_LABELS: Record<string, string> = {
  shadowsocks: 'Shadowsocks',
  vmess: 'VMess',
  vless: 'VLESS',
  trojan: 'Trojan',
  hysteria: 'Hysteria',
  hysteria2: 'Hysteria2',
  tuic: 'TUIC',
  anytls: 'AnyTLS',
  naive: 'NaiveProxy',
  http: 'HTTP',
  socks: 'SOCKS5',
  wireguard: 'WireGuard',
  ssh: 'SSH',
  shadowtls: 'ShadowTLS',
}

const INPUT_CLASS = 'w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50'

function Field({ label, value, onChange, type = 'text', placeholder, disabled, multiline }: {
  label: string
  value: string
  onChange: (value: string) => void
  type?: 'text' | 'password' | 'number'
  placeholder?: string
  disabled?: boolean
  multiline?: boolean
}) {
  return (
    <label className="space-y-1.5 min-w-0">
      <span className="block text-xs font-medium text-[var(--text-muted)]">{label}</span>
      {multiline ? (
        <textarea
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          disabled={disabled}
          rows={4}
          className={`${INPUT_CLASS} resize-y font-mono`}
        />
      ) : (
        <input
          type={type}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          disabled={disabled}
          className={INPUT_CLASS}
        />
      )}
    </label>
  )
}

function SelectField({ label, value, options, onChange, disabled }: {
  label: string
  value: string
  options: Array<{ value: string; label: string }>
  onChange: (value: string) => void
  disabled?: boolean
}) {
  return (
    <div className="space-y-1.5 min-w-0">
      <span className="block text-xs font-medium text-[var(--text-muted)]">{label}</span>
      <AppSelect
        value={value}
        options={options}
        onValueChange={onChange}
        disabled={disabled}
        ariaLabel={label}
      />
    </div>
  )
}

function ToggleField({ label, description, checked, onChange, disabled, tone = 'default' }: {
  label: string
  description?: string
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  tone?: 'default' | 'warning'
}) {
  return (
    <div className={`flex items-center justify-between gap-4 rounded-xl border px-3 py-2.5 ${tone === 'warning' ? 'border-amber-500/30 bg-amber-500/5' : 'border-[var(--border-secondary)] bg-[var(--bg-elevated)]/45'}`}>
      <div className="min-w-0">
        <p className="text-sm font-medium text-[var(--text-primary)]">{label}</p>
        {description && <p className="mt-0.5 text-xs leading-5 text-[var(--text-faint)]">{description}</p>}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        disabled={disabled}
        className={`w-11 h-6 shrink-0 rounded-full transition-colors ${checked ? 'bg-[var(--accent-primary)]' : 'bg-[var(--border-secondary)]'} disabled:opacity-50`}
      >
        <span className={`block w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${checked ? 'translate-x-5' : 'translate-x-0.5'}`} />
      </button>
    </div>
  )
}

function Section({ title, code, children }: { title: string; code: string; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border border-[var(--border-secondary)] bg-[var(--bg-secondary)]/35 overflow-hidden">
      <div className="flex items-center gap-2.5 px-4 py-3 border-b border-[var(--border-secondary)]">
        <span className="inline-flex min-w-8 h-6 px-1.5 items-center justify-center rounded-md bg-[var(--accent-muted)] font-mono text-[10px] font-bold text-[var(--accent-primary)]">{code}</span>
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">{title}</h3>
      </div>
      <div className="p-4 space-y-3">{children}</div>
    </section>
  )
}

function optionalNumber(value: string): number | undefined {
  const trimmed = value.trim()
  if (!trimmed) return undefined
  const number = Number(trimmed)
  return Number.isFinite(number) ? number : undefined
}

function stringList(value: string): string[] | undefined {
  const values = value.split(',').map((item) => item.trim()).filter(Boolean)
  return values.length ? values : undefined
}

function lineList(value: string): string[] | undefined {
  const values = value.split('\n').map((item) => item.trim()).filter(Boolean)
  return values.length ? values : undefined
}

function firstString(value: string | string[] | undefined): string {
  return Array.isArray(value) ? value[0] ?? '' : value ?? ''
}

function obfsObject(node: SingBoxOutbound) {
  return typeof node.obfs === 'object' && node.obfs !== null ? node.obfs : { type: typeof node.obfs === 'string' ? node.obfs : '' }
}

function udpOverTcpEnabled(node: SingBoxOutbound): boolean {
  return node.udp_over_tcp === true || (typeof node.udp_over_tcp === 'object' && node.udp_over_tcp?.enabled === true)
}

function formatHeaders(headers: Record<string, string | string[]> | undefined): string {
  return Object.entries(headers ?? {}).map(([key, value]) => `${key}: ${Array.isArray(value) ? value.join(', ') : value}`).join('\n')
}

function parseHeaders(value: string): Record<string, string> | undefined {
  const headers: Record<string, string> = {}
  for (const line of value.split('\n')) {
    const separator = line.indexOf(':')
    if (separator <= 0) continue
    const key = line.slice(0, separator).trim()
    const headerValue = line.slice(separator + 1).trim()
    if (key && headerValue) headers[key] = headerValue
  }
  return Object.keys(headers).length ? headers : undefined
}

function ProtocolFields({ node, onChange, disabled }: {
  node: SingBoxOutbound
  onChange: (node: SingBoxOutbound) => void
  disabled: boolean
}) {
  const type = node.type ?? ''
  const set = <K extends keyof SingBoxOutbound>(key: K, value: SingBoxOutbound[K]) => onChange({ ...node, [key]: value })
  const twoColumns = 'grid grid-cols-1 sm:grid-cols-2 gap-3'

  if (type === 'shadowsocks') {
    return <>
      <div className={twoColumns}>
        <SelectField label="加密方式" value={node.method ?? 'aes-128-gcm'} disabled={disabled} onChange={(value) => set('method', value)} options={[
          { value: '2022-blake3-aes-128-gcm', label: '2022 BLAKE3 AES-128-GCM' },
          { value: '2022-blake3-aes-256-gcm', label: '2022 BLAKE3 AES-256-GCM' },
          { value: '2022-blake3-chacha20-poly1305', label: '2022 BLAKE3 ChaCha20-Poly1305' },
          { value: 'aes-128-gcm', label: 'AES-128-GCM' }, { value: 'aes-192-gcm', label: 'AES-192-GCM' },
          { value: 'aes-256-gcm', label: 'AES-256-GCM' }, { value: 'chacha20-ietf-poly1305', label: 'ChaCha20-IETF-Poly1305' },
          { value: 'xchacha20-ietf-poly1305', label: 'XChaCha20-IETF-Poly1305' },
          { value: 'aes-128-ctr', label: 'AES-128-CTR' }, { value: 'aes-192-ctr', label: 'AES-192-CTR' },
          { value: 'aes-256-ctr', label: 'AES-256-CTR' }, { value: 'aes-128-cfb', label: 'AES-128-CFB' },
          { value: 'aes-192-cfb', label: 'AES-192-CFB' }, { value: 'aes-256-cfb', label: 'AES-256-CFB' },
          { value: 'rc4-md5', label: 'RC4-MD5' }, { value: 'chacha20-ietf', label: 'ChaCha20-IETF' },
          { value: 'xchacha20', label: 'XChaCha20' }, { value: 'none', label: 'None' },
        ]} />
        <Field label="密码" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value)} />
        <Field label="插件（可选）" value={node.plugin ?? ''} disabled={disabled} onChange={(value) => set('plugin', value || undefined)} />
        <Field label="插件参数（可选）" value={node.plugin_opts ?? ''} disabled={disabled} onChange={(value) => set('plugin_opts', value || undefined)} />
      </div>
      <ToggleField label="UDP over TCP" checked={udpOverTcpEnabled(node)} disabled={disabled} onChange={(enabled) => set('udp_over_tcp', enabled ? { enabled: true } : undefined)} />
    </>
  }

  if (type === 'vmess' || type === 'vless') {
    return <div className={twoColumns}>
      <Field label="UUID" value={node.uuid ?? ''} disabled={disabled} onChange={(value) => set('uuid', value)} />
      {type === 'vmess' ? (
        <SelectField label="加密方式" value={node.security ?? 'auto'} disabled={disabled} onChange={(value) => set('security', value)} options={[
          { value: 'auto', label: 'Auto' }, { value: 'aes-128-gcm', label: 'AES-128-GCM' },
          { value: 'chacha20-poly1305', label: 'ChaCha20-Poly1305' }, { value: 'none', label: 'None' },
        ]} />
      ) : (
        <SelectField label="Flow" value={node.flow ?? ''} disabled={disabled} onChange={(value) => set('flow', value || undefined)} options={[
          { value: '', label: '无' }, { value: 'xtls-rprx-vision', label: 'xtls-rprx-vision' },
        ]} />
      )}
      <SelectField label="Packet Encoding" value={node.packet_encoding ?? 'xudp'} disabled={disabled} onChange={(value) => set('packet_encoding', value || undefined)} options={[
        { value: '', label: '无' }, { value: 'xudp', label: 'xudp' }, { value: 'packet', label: 'packet' },
      ]} />
      {type === 'vmess' && <Field label="Alter ID" type="number" value={String(node.alter_id ?? 0)} disabled={disabled} onChange={(value) => set('alter_id', optionalNumber(value))} />}
    </div>
  }

  if (type === 'trojan') {
    return <Field label="密码" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value)} />
  }

  if (type === 'hysteria2') {
    const obfs = obfsObject(node)
    return <div className="space-y-3">
      <div className={twoColumns}>
        <Field label="密码" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value)} />
        <Field label="端口跳跃" value={node.server_ports?.[0] ?? ''} placeholder="20000:30000" disabled={disabled} onChange={(value) => set('server_ports', value ? [value] : undefined)} />
        <SelectField label="混淆方式" value={obfs.type ?? ''} disabled={disabled} onChange={(value) => set('obfs', value ? { ...obfs, type: value } : undefined)} options={[
          { value: '', label: '不使用' }, { value: 'salamander', label: 'Salamander' },
        ]} />
        {obfs.type === 'salamander' && <Field label="混淆密码" type="password" value={obfs.password ?? ''} disabled={disabled} onChange={(value) => set('obfs', { ...obfs, password: value })} />}
        <Field label="上传带宽 Mbps" type="number" value={String(node.up_mbps ?? '')} disabled={disabled} onChange={(value) => set('up_mbps', optionalNumber(value))} />
        <Field label="下载带宽 Mbps" type="number" value={String(node.down_mbps ?? '')} disabled={disabled} onChange={(value) => set('down_mbps', optionalNumber(value))} />
      </div>
      <ToggleField label="禁用 SNI" checked={node.tls?.disable_sni === true} disabled={disabled} onChange={(checked) => set('tls', { ...(node.tls ?? { enabled: true }), enabled: true, disable_sni: checked || undefined })} />
    </div>
  }

  if (type === 'tuic') {
    return <div className="space-y-3">
      <div className={twoColumns}>
        <Field label="UUID" value={node.uuid ?? ''} disabled={disabled} onChange={(value) => set('uuid', value)} />
        <Field label="密码" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value)} />
        <SelectField label="拥塞控制" value={node.congestion_control ?? 'bbr'} disabled={disabled} onChange={(value) => set('congestion_control', value)} options={[
          { value: 'bbr', label: 'BBR' }, { value: 'cubic', label: 'Cubic' }, { value: 'new_reno', label: 'New Reno' },
        ]} />
        <SelectField label="UDP 中继" value={node.udp_relay_mode ?? 'native'} disabled={disabled} onChange={(value) => set('udp_relay_mode', value)} options={[
          { value: 'native', label: 'Native' }, { value: 'quic', label: 'QUIC' },
        ]} />
        <Field label="心跳间隔" value={node.heartbeat ?? '3s'} disabled={disabled} onChange={(value) => set('heartbeat', value || undefined)} />
      </div>
      <ToggleField label="0-RTT 握手" checked={node.zero_rtt_handshake === true} disabled={disabled} onChange={(checked) => set('zero_rtt_handshake', checked)} />
      <ToggleField label="禁用 SNI" checked={node.tls?.disable_sni === true} disabled={disabled} onChange={(checked) => set('tls', { ...(node.tls ?? { enabled: true }), enabled: true, disable_sni: checked || undefined })} />
    </div>
  }

  if (type === 'naive') {
    const network = Array.isArray(node.network) ? node.network[0] : node.network
    return <div className="space-y-3">
      <div className={twoColumns}>
        <Field label="用户名" value={node.username ?? ''} disabled={disabled} onChange={(value) => set('username', value || undefined)} />
        <Field label="密码" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value || undefined)} />
        <SelectField label="传输协议" value={node.quic === true || network === 'quic' ? 'quic' : 'h2'} disabled={disabled} onChange={(value) => onChange({ ...node, network: [value], quic: value === 'quic' })} options={[
          { value: 'h2', label: 'HTTP/2' }, { value: 'quic', label: 'QUIC' },
        ]} />
        <Field label="不安全并发数" type="number" value={String(node.insecure_concurrency ?? '')} disabled={disabled} onChange={(value) => set('insecure_concurrency', optionalNumber(value))} />
        <SelectField label="拥塞控制" value={node.congestion_control ?? ''} disabled={disabled} onChange={(value) => set('congestion_control', value || undefined)} options={[
          { value: '', label: '默认' }, { value: 'bbr', label: 'BBR' }, { value: 'cubic', label: 'Cubic' }, { value: 'new_reno', label: 'New Reno' },
        ]} />
      </div>
      <Field label="额外请求头" value={formatHeaders(node.extra_headers)} placeholder="User-Agent: naive" disabled={disabled} multiline onChange={(value) => set('extra_headers', parseHeaders(value))} />
      <ToggleField label="UDP over TCP" checked={udpOverTcpEnabled(node)} disabled={disabled} onChange={(enabled) => set('udp_over_tcp', enabled ? { enabled: true } : undefined)} />
    </div>
  }

  if (type === 'anytls') {
    return <div className={twoColumns}>
      <Field label="密码" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value)} />
      <Field label="空闲会话检查间隔" value={node.idle_session_check_interval ?? '30s'} disabled={disabled} onChange={(value) => set('idle_session_check_interval', value || undefined)} />
      <Field label="空闲会话超时" value={node.idle_session_timeout ?? '30s'} disabled={disabled} onChange={(value) => set('idle_session_timeout', value || undefined)} />
      <Field label="最小空闲会话数" type="number" value={String(node.min_idle_session ?? 0)} disabled={disabled} onChange={(value) => set('min_idle_session', optionalNumber(value))} />
    </div>
  }

  if (type === 'socks' || type === 'http') {
    return <div className={twoColumns}>
      {type === 'socks' && <SelectField label="SOCKS 版本" value={String(node.version ?? '5')} disabled={disabled} onChange={(value) => set('version', value)} options={[
        { value: '4', label: '4' }, { value: '4a', label: '4a' }, { value: '5', label: '5' },
      ]} />}
      <Field label="用户名（可选）" value={node.username ?? ''} disabled={disabled} onChange={(value) => set('username', value || undefined)} />
      <Field label="密码（可选）" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value || undefined)} />
    </div>
  }

  if (type === 'hysteria') {
    return <div className={twoColumns}>
      <Field label="认证字符串" type="password" value={node.auth_str ?? ''} disabled={disabled} onChange={(value) => set('auth_str', value || undefined)} />
      <Field label="上传带宽 Mbps" type="number" value={String(node.up_mbps ?? '')} disabled={disabled} onChange={(value) => set('up_mbps', optionalNumber(value))} />
      <Field label="下载带宽 Mbps" type="number" value={String(node.down_mbps ?? '')} disabled={disabled} onChange={(value) => set('down_mbps', optionalNumber(value))} />
      <Field label="混淆方式" value={typeof node.obfs === 'string' ? node.obfs : node.obfs?.type ?? ''} disabled={disabled} onChange={(value) => set('obfs', value || undefined)} />
      <Field label="端口跳跃间隔" value={node.hop_interval ?? '10s'} disabled={disabled} onChange={(value) => set('hop_interval', value || undefined)} />
    </div>
  }

  if (type === 'wireguard') {
    const peer = node.peers?.[0] ?? {}
    const updatePeer = (patch: typeof peer) => set('peers', [{ ...peer, ...patch }])
    return <div className={twoColumns}>
      <Field label="服务器地址" value={peer.server ?? ''} disabled={disabled} onChange={(value) => updatePeer({ server: value })} />
      <Field label="服务器端口" type="number" value={String(peer.server_port ?? '')} disabled={disabled} onChange={(value) => updatePeer({ server_port: optionalNumber(value) })} />
      <Field label="私钥" type="password" value={firstString(node.private_key)} disabled={disabled} onChange={(value) => set('private_key', value ? [value] : undefined)} />
      <Field label="对端公钥" value={peer.public_key ?? ''} disabled={disabled} onChange={(value) => updatePeer({ public_key: value })} />
      <Field label="预共享密钥" type="password" value={peer.pre_shared_key ?? ''} disabled={disabled} onChange={(value) => updatePeer({ pre_shared_key: value || undefined })} />
      <Field label="本地地址" value={node.local_address?.join(', ') ?? ''} disabled={disabled} onChange={(value) => set('local_address', stringList(value))} />
      <Field label="允许的 IP" value={peer.allowed_ips?.join(', ') ?? ''} disabled={disabled} onChange={(value) => updatePeer({ allowed_ips: stringList(value) })} />
      <Field label="保活间隔" type="number" value={String(peer.persistent_keepalive_interval ?? '')} disabled={disabled} onChange={(value) => updatePeer({ persistent_keepalive_interval: optionalNumber(value) })} />
      <Field label="MTU" type="number" value={String(node.mtu ?? 1420)} disabled={disabled} onChange={(value) => set('mtu', optionalNumber(value))} />
      <Field label="Reserved" value={peer.reserved?.join(', ') ?? ''} disabled={disabled} onChange={(value) => updatePeer({ reserved: stringList(value)?.map(Number).filter(Number.isFinite) })} />
    </div>
  }

  if (type === 'ssh') {
    return <div className={twoColumns}>
      <Field label="用户名" value={node.user ?? ''} disabled={disabled} onChange={(value) => set('user', value)} />
      <Field label="密码（可选）" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value || undefined)} />
      <Field label="私钥" value={firstString(node.private_key)} disabled={disabled} multiline onChange={(value) => set('private_key', value ? [value] : undefined)} />
      <Field label="私钥口令" type="password" value={node.private_key_passphrase ?? ''} disabled={disabled} onChange={(value) => set('private_key_passphrase', value || undefined)} />
      <Field label="主机密钥" value={node.host_key?.join('\n') ?? ''} disabled={disabled} multiline onChange={(value) => set('host_key', lineList(value))} />
    </div>
  }

  if (type === 'shadowtls') {
    return <div className={twoColumns}>
      <SelectField label="ShadowTLS 版本" value={String(node.version ?? 3)} disabled={disabled} onChange={(value) => set('version', Number(value))} options={[
        { value: '1', label: '1' }, { value: '2', label: '2' }, { value: '3', label: '3' },
      ]} />
      <Field label="密码" type="password" value={node.password ?? ''} disabled={disabled} onChange={(value) => set('password', value)} />
    </div>
  }

  return <p className="text-sm text-[var(--text-faint)]">该协议保留原始字段，可编辑通用、TLS、传输和策略参数。</p>
}

function TransportFields({ node, onChange, disabled }: { node: SingBoxOutbound; onChange: (node: SingBoxOutbound) => void; disabled: boolean }) {
  if (!['vmess', 'vless', 'trojan'].includes(node.type ?? '')) return null
  const transport = node.transport ?? { type: 'tcp' }
  const update = (patch: Partial<OutboundTransport>) => onChange({ ...node, transport: { ...transport, ...patch } })
  const type = transport.type ?? 'tcp'
  const hostValue = type === 'ws'
    ? transport.headers?.Host ?? transport.headers?.host ?? ''
    : transport.host?.join(', ') ?? ''
  const hostText = Array.isArray(hostValue) ? hostValue.join(', ') : hostValue
  const updateHost = (value: string) => {
    if (type === 'ws') {
      const headers = { ...(transport.headers ?? {}) }
      delete headers.Host
      delete headers.host
      if (value.trim()) headers.Host = value.trim()
      update({ headers: Object.keys(headers).length ? headers : undefined, host: undefined })
    } else {
      update({ host: stringList(value) })
    }
  }
  const pathBased = ['http', 'h2', 'ws', 'httpupgrade', 'xhttp'].includes(type)

  return <Section title="传输设置" code="NET">
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
      <SelectField label="传输协议" value={type} disabled={disabled} onChange={(value) => update({ type: value })} options={[
        { value: 'tcp', label: 'TCP' }, { value: 'http', label: 'HTTP' }, { value: 'ws', label: 'WebSocket' },
        { value: 'grpc', label: 'gRPC' }, { value: 'quic', label: 'QUIC' }, { value: 'httpupgrade', label: 'HTTPUpgrade' },
        { value: 'xhttp', label: 'XHTTP' },
      ]} />
      {pathBased && <Field label="路径" value={transport.path ?? '/'} disabled={disabled} onChange={(value) => update({ path: value })} />}
      {pathBased && <Field label="Host" value={hostText} disabled={disabled} onChange={updateHost} />}
      {type === 'grpc' && <Field label="Service Name" value={transport.service_name ?? ''} disabled={disabled} onChange={(value) => update({ service_name: value || undefined })} />}
      {type === 'ws' && <Field label="Max Early Data" type="number" value={String(transport.max_early_data ?? '')} disabled={disabled} onChange={(value) => update({ max_early_data: optionalNumber(value) })} />}
      {type === 'ws' && <Field label="Early Data Header" value={transport.early_data_header_name ?? ''} disabled={disabled} onChange={(value) => update({ early_data_header_name: value || undefined })} />}
      {type === 'xhttp' && <SelectField label="XHTTP 模式" value={transport.mode ?? 'auto'} disabled={disabled} onChange={(value) => update({ mode: value })} options={[
        { value: 'auto', label: 'Auto' }, { value: 'packet-up', label: 'Packet Up' }, { value: 'stream-up', label: 'Stream Up' },
      ]} />}
      {type === 'xhttp' && <Field label="XPadding Bytes" value={transport.x_padding_bytes ?? ''} disabled={disabled} onChange={(value) => update({ x_padding_bytes: value || undefined })} />}
      {type === 'xhttp' && <Field label="单次 POST 最大字节" type="number" value={String(transport.sc_max_each_post_bytes ?? '')} disabled={disabled} onChange={(value) => update({ sc_max_each_post_bytes: optionalNumber(value) })} />}
      {type === 'xhttp' && <Field label="POST 最小间隔 ms" type="number" value={String(transport.sc_min_posts_interval_ms ?? '')} disabled={disabled} onChange={(value) => update({ sc_min_posts_interval_ms: optionalNumber(value) })} />}
      {type === 'xhttp' && <Field label="最大缓冲 POST 数" type="number" value={String(transport.sc_max_buffered_posts ?? '')} disabled={disabled} onChange={(value) => update({ sc_max_buffered_posts: optionalNumber(value) })} />}
    </div>
    {type === 'xhttp' && <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
      <ToggleField label="No gRPC Header" checked={transport.no_grpc_header === true} disabled={disabled} onChange={(checked) => update({ no_grpc_header: checked })} />
      <ToggleField label="No SSE Header" checked={transport.no_sse_header === true} disabled={disabled} onChange={(checked) => update({ no_sse_header: checked })} />
    </div>}
  </Section>
}

function TlsFields({ node, onChange, disabled }: { node: SingBoxOutbound; onChange: (node: SingBoxOutbound) => void; disabled: boolean }) {
  if (['wireguard', 'ssh', 'shadowsocks'].includes(node.type ?? '')) return null
  const tls = node.tls ?? { enabled: false }
  const intrinsic = ['hysteria2', 'hysteria', 'tuic', 'naive', 'anytls'].includes(node.type ?? '')
  const security = tls.reality?.enabled ? 'reality' : intrinsic || tls.enabled ? 'tls' : 'none'
  const update = (patch: Partial<OutboundTls>) => onChange({ ...node, tls: { ...tls, ...patch } })
  const setSecurity = (value: string) => {
    if (value === 'none') onChange({ ...node, tls: { ...tls, enabled: false, reality: undefined } })
    if (value === 'tls') onChange({ ...node, tls: { ...tls, enabled: true, reality: undefined } })
    if (value === 'reality') onChange({ ...node, tls: { ...tls, enabled: true, reality: { ...(tls.reality ?? {}), enabled: true } } })
  }

  return <Section title="TLS 设置" code="TLS">
    {!intrinsic && <SelectField label="安全模式" value={security} disabled={disabled} onChange={setSecurity} options={[
      { value: 'none', label: '不使用' }, { value: 'tls', label: 'TLS' }, { value: 'reality', label: 'Reality' },
    ]} />}
    {security !== 'none' && <>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label="服务器名称 SNI" value={tls.server_name ?? ''} disabled={disabled} onChange={(value) => update({ server_name: value || undefined })} />
        {node.type !== 'naive' && <Field label="ALPN" value={tls.alpn?.join(', ') ?? ''} disabled={disabled} onChange={(value) => update({ alpn: stringList(value) })} />}
        {node.type !== 'naive' && <SelectField label="uTLS 指纹" value={tls.utls?.fingerprint ?? ''} disabled={disabled} onChange={(value) => update({ utls: value ? { enabled: true, fingerprint: value } : undefined })} options={[
          { value: '', label: '不使用' }, { value: 'chrome', label: 'Chrome' }, { value: 'firefox', label: 'Firefox' },
          { value: 'safari', label: 'Safari' }, { value: 'ios', label: 'iOS' }, { value: 'android', label: 'Android' },
          { value: 'edge', label: 'Edge' }, { value: '360', label: '360' }, { value: 'qq', label: 'QQ' },
          { value: 'random', label: 'Random' }, { value: 'randomized', label: 'Randomized' },
        ]} />}
      </div>
      {node.type !== 'naive' && <ToggleField label="跳过证书验证" description="仅用于自签名证书" checked={tls.insecure === true} disabled={disabled} onChange={(checked) => update({ insecure: checked })} />}
      <Field label="CA 证书 PEM" value={tls.ca?.join('\n') ?? ''} disabled={disabled} multiline onChange={(value) => update({ ca: lineList(value) })} />
      {node.type !== 'naive' && <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label="客户端证书 PEM" value={tls.certificate?.join('\n') ?? ''} disabled={disabled} multiline onChange={(value) => update({ certificate: lineList(value) })} />
        <Field label="客户端密钥 PEM" value={tls.key?.join('\n') ?? ''} disabled={disabled} multiline onChange={(value) => update({ key: lineList(value) })} />
      </div>}
      {security === 'reality' && <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label="Reality 公钥" value={tls.reality?.public_key ?? ''} disabled={disabled} onChange={(value) => update({ reality: { ...(tls.reality ?? { enabled: true }), enabled: true, public_key: value } })} />
        <Field label="Reality Short ID" value={tls.reality?.short_id ?? ''} disabled={disabled} onChange={(value) => update({ reality: { ...(tls.reality ?? { enabled: true }), enabled: true, short_id: value || undefined } })} />
      </div>}
      <ToggleField label="启用 ECH" checked={tls.ech?.enabled === true} disabled={disabled} onChange={(checked) => update({ ech: { ...(tls.ech ?? {}), enabled: checked } })} />
      {tls.ech?.enabled && <Field label="ECH 配置" value={tls.ech.config?.join('\n') ?? ''} disabled={disabled} multiline onChange={(value) => update({ ech: { ...(tls.ech ?? { enabled: true }), enabled: true, config: lineList(value) } })} />}
    </>}
  </Section>
}

function MultiplexFields({ node, onChange, disabled }: { node: SingBoxOutbound; onChange: (node: SingBoxOutbound) => void; disabled: boolean }) {
  if (!['vmess', 'vless', 'trojan', 'shadowsocks'].includes(node.type ?? '')) return null
  const multiplex = node.multiplex ?? { enabled: false }
  const update = (patch: Partial<OutboundMultiplex>) => onChange({ ...node, multiplex: { ...multiplex, ...patch } })
  return <Section title="多路复用" code="MUX">
    <ToggleField label="启用多路复用" checked={multiplex.enabled === true} disabled={disabled} onChange={(checked) => update({ enabled: checked })} />
    {multiplex.enabled && <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
      <SelectField label="协议" value={multiplex.protocol ?? 'h2mux'} disabled={disabled} onChange={(value) => update({ protocol: value })} options={[
        { value: 'h2mux', label: 'h2mux' }, { value: 'smux', label: 'smux' }, { value: 'yamux', label: 'yamux' },
      ]} />
      <Field label="最大连接数" type="number" value={String(multiplex.max_connections ?? 5)} disabled={disabled} onChange={(value) => update({ max_connections: optionalNumber(value) })} />
      <Field label="最小流数" type="number" value={String(multiplex.min_streams ?? '')} disabled={disabled} onChange={(value) => update({ min_streams: optionalNumber(value) })} />
      <Field label="最大流数" type="number" value={String(multiplex.max_streams ?? '')} disabled={disabled} onChange={(value) => update({ max_streams: optionalNumber(value) })} />
    </div>}
    {multiplex.enabled && <ToggleField label="启用填充" checked={multiplex.padding === true} disabled={disabled} onChange={(checked) => update({ padding: checked })} />}
  </Section>
}

export function NodeDetailModal({ isOpen, onClose, node, profileId, onSave, onExport }: NodeDetailModalProps) {
  const [draft, setDraft] = useState<SingBoxOutbound | null>(null)
  const [frontProxyNodes, setFrontProxyNodes] = useState<NodeWithProfile[]>([])
  const [autoSelectionEligible, setAutoSelectionEligible] = useState(true)
  const [meteredProtected, setMeteredProtected] = useState(false)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)
  const [copied, setCopied] = useState(false)
  const [isExporting, setIsExporting] = useState(false)
  const { setManagedTimeout } = useManagedTimeouts()

  useEffect(() => {
    if (!isOpen || !node) return
    const cloned = toEditableNode(node)
    const policy = nodePolicyState(cloned)
    setDraft(cloned)
    setAutoSelectionEligible(policy.autoSelectionEligible)
    setMeteredProtected(policy.meteredProtected)
    setError('')
    setCopied(false)
    void window.api.node.listAll().then(setFrontProxyNodes).catch(() => setFrontProxyNodes([]))
  }, [isOpen, node])

  const frontProxyOptions = useMemo(() => {
    const counts = new Map<string, number>()
    for (const item of frontProxyNodes) {
      if (!item.tag?.trim()) continue
      const reference = makeNodeReference(item)
      counts.set(reference, (counts.get(reference) ?? 0) + 1)
    }
    return frontProxyNodes.flatMap((item) => {
      if (!item.tag?.trim()) return []
      const reference = makeNodeReference(item)
      if (counts.get(reference) !== 1) return []
      if (item.sourceProfileId === profileId && item.tag === node?.tag) return []
      if (item.x_kunbox_metered_protected === true) return []
      return [{ value: reference, label: `${item.sourceProfileName} / ${item.tag}` }]
    })
  }, [frontProxyNodes, node?.tag, profileId])

  if (!node || !draft) return null

  const originalTag = node.tag ?? ''
  const protocol = draft.type?.toLowerCase() ?? ''
  const protocolLabel = PROTOCOL_LABELS[protocol] ?? (protocol.toUpperCase() || 'Unknown')
  const disabled = saving || isExporting
  const detourSelectValue = draft.detour
    ? (draft.detour.includes('::') ? draft.detour : `${profileId ?? ''}::${draft.detour}`)
    : ''
  const detourSelectOptions = detourSelectValue && !frontProxyOptions.some((option) => option.value === detourSelectValue)
    ? [{ value: detourSelectValue, label: `已保存：${draft.detour}` }, ...frontProxyOptions]
    : frontProxyOptions

  const handleSave = async () => {
    if (!profileId) {
      setError('当前没有活动配置')
      return
    }
    const normalized = applyNodePolicies(draft, autoSelectionEligible, meteredProtected)
    const validationError = validateNodeForSave(normalized)
    if (validationError) {
      setError(validationError)
      return
    }
    setSaving(true)
    setError('')
    try {
      await onSave(originalTag, normalized)
      onClose()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      setSaving(false)
    }
  }

  const handleExport = async () => {
    if (!originalTag || !onExport) return
    setIsExporting(true)
    try {
      await onExport(originalTag)
      setCopied(true)
      setManagedTimeout(() => setCopied(false), 2000)
    } finally {
      setIsExporting(false)
    }
  }

  return (
    <Modal
      isOpen={isOpen}
      onClose={disabled ? () => undefined : onClose}
      title="编辑节点"
      maxWidth="max-w-4xl"
      footer={<>
        {onExport && <ModalButton variant="secondary" onClick={handleExport} disabled={disabled}>
          {isExporting ? <Loader2 className="w-4 h-4 animate-spin" /> : copied ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
          {copied ? '已复制' : '复制链接'}
        </ModalButton>}
        <ModalButton variant="secondary" onClick={onClose} disabled={disabled}>取消</ModalButton>
        <ModalButton variant="primary" onClick={handleSave} disabled={disabled}>
          {saving ? <Loader2 className="w-4 h-4 animate-spin" /> : <Save className="w-4 h-4" />}
          保存更改
        </ModalButton>
      </>}
    >
      <div className="space-y-4">
        <div className="flex items-center justify-between gap-4 rounded-xl border border-[var(--accent-primary)]/25 bg-[var(--accent-muted)]/35 px-4 py-3">
          <div className="min-w-0">
            <p className="text-xs text-[var(--text-faint)]">sing-box 出站配置</p>
            <p className="mt-0.5 truncate text-base font-semibold text-[var(--text-primary)]">{draft.tag || '未命名节点'}</p>
          </div>
          <span className="shrink-0 rounded-lg border border-[var(--accent-primary)]/30 bg-[var(--bg-elevated)] px-3 py-1.5 font-mono text-xs font-bold text-[var(--accent-primary)]">{protocolLabel}</span>
        </div>

        <Section title="基本信息" code="ID">
          <Field label="节点名称" value={draft.tag ?? ''} disabled={disabled} onChange={(value) => { setDraft({ ...draft, tag: value }); setError('') }} />
          {protocol !== 'wireguard' && <div className="grid grid-cols-[minmax(0,1fr)_140px] gap-3">
            <Field label="服务器地址" value={draft.server ?? ''} disabled={disabled} onChange={(value) => setDraft({ ...draft, server: value })} />
            <Field label="端口" type="number" value={String(draft.server_port ?? '')} disabled={disabled} onChange={(value) => setDraft({ ...draft, server_port: optionalNumber(value) })} />
          </div>}
        </Section>

        <Section title={`${protocolLabel} 参数`} code={protocolLabel.slice(0, 4).toUpperCase()}>
          <ProtocolFields node={draft} onChange={setDraft} disabled={disabled} />
        </Section>

        <TransportFields node={draft} onChange={setDraft} disabled={disabled} />
        <TlsFields node={draft} onChange={setDraft} disabled={disabled} />
        <MultiplexFields node={draft} onChange={setDraft} disabled={disabled} />

        <Section title="节点策略" code="POL">
          <SelectField label="前置代理" value={detourSelectValue} disabled={disabled} onChange={(value) => setDraft({ ...draft, detour: value || undefined })} options={[
            { value: '', label: '不使用前置代理' }, ...detourSelectOptions,
          ]} />
          <ToggleField
            label="高价计费节点保护"
            description="仅在明确手动选中时进入运行配置，并阻止分流、DNS、链式代理和后台探测使用"
            checked={meteredProtected}
            disabled={disabled}
            tone="warning"
            onChange={(checked) => { setMeteredProtected(checked); if (checked) setAutoSelectionEligible(false) }}
          />
          <ToggleField
            label="参与自动探测与切换"
            description="关闭后跳过后台健康探测和自动切换，仍可手动选择与测速"
            checked={autoSelectionEligible}
            disabled={disabled}
            onChange={(checked) => { setAutoSelectionEligible(checked); if (checked) setMeteredProtected(false) }}
          />
          <ToggleField label="TCP Fast Open" checked={draft.tcp_fast_open === true} disabled={disabled} onChange={(checked) => setDraft({ ...draft, tcp_fast_open: checked })} />
        </Section>

        {error && <div className="rounded-xl border border-[var(--status-error)]/30 bg-[var(--status-error)]/10 px-4 py-3 text-sm text-[var(--status-error)]">{error}</div>}
      </div>
    </Modal>
  )
}
