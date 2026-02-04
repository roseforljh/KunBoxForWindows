import { useState, useEffect } from 'react'
import { Modal, ModalButton } from './Modal'
import { Loader2, Copy, Check } from 'lucide-react'
import type { SingBoxOutbound } from '@shared/types'

interface NodeDetailModalProps {
  isOpen: boolean
  onClose: () => void
  node: SingBoxOutbound | null
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
  http: 'HTTP',
  socks: 'SOCKS5'
}

export function NodeDetailModal({
  isOpen,
  onClose,
  node,
  onExport
}: NodeDetailModalProps) {
  const [copied, setCopied] = useState(false)
  const [isExporting, setIsExporting] = useState(false)

  useEffect(() => {
    if (!isOpen) {
      setCopied(false)
    }
  }, [isOpen])

  const handleExport = async () => {
    if (!node?.tag || !onExport) return
    setIsExporting(true)
    try {
      await onExport(node.tag)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } finally {
      setIsExporting(false)
    }
  }

  if (!node) return null

  const protocolLabel = PROTOCOL_LABELS[node.type?.toLowerCase() || ''] || node.type?.toUpperCase() || 'Unknown'

  const fields = [
    { label: '名称', value: node.tag },
    { label: '协议', value: protocolLabel },
    { label: '服务器', value: node.server },
    { label: '端口', value: node.server_port?.toString() },
    { label: 'UUID', value: node.uuid, hide: !node.uuid },
    { label: '密码', value: node.password ? '••••••••' : undefined, hide: !node.password },
    { label: '加密方式', value: node.method, hide: !node.method },
    { label: 'Flow', value: node.flow, hide: !node.flow }
  ].filter(f => !f.hide && f.value)

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="节点详情"
      maxWidth="max-w-md"
      footer={
        <>
          <ModalButton variant="secondary" onClick={onClose}>
            关闭
          </ModalButton>
          {onExport && (
            <ModalButton
              variant="primary"
              onClick={handleExport}
              disabled={isExporting}
            >
              {isExporting ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : copied ? (
                <>
                  <Check className="w-4 h-4" />
                  已复制
                </>
              ) : (
                <>
                  <Copy className="w-4 h-4" />
                  复制链接
                </>
              )}
            </ModalButton>
          )}
        </>
      }
    >
      <div className="space-y-3">
        {fields.map((field) => (
          <div
            key={field.label}
            className="flex items-start justify-between py-2 border-b border-[var(--border-secondary)] last:border-0"
          >
            <span className="text-sm text-[var(--text-muted)] shrink-0">
              {field.label}
            </span>
            <span className="text-sm text-[var(--text-primary)] text-right break-all ml-4 font-mono">
              {field.value}
            </span>
          </div>
        ))}
      </div>
    </Modal>
  )
}
