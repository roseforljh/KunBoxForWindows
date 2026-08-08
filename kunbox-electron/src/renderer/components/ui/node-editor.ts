import type { NodeWithProfile, SingBoxOutbound } from '@shared/types'

export const NODE_POLICY_KEYS = {
  autoSelectionEligible: 'x_kunbox_auto_selection_eligible',
  meteredProtected: 'x_kunbox_metered_protected',
} as const

export function makeNodeReference(node: Pick<NodeWithProfile, 'sourceProfileId' | 'tag'>): string {
  return `${node.sourceProfileId}::${node.tag ?? ''}`
}

export function applyNodePolicies(
  node: SingBoxOutbound,
  autoSelectionEligible: boolean,
  meteredProtected: boolean,
): SingBoxOutbound {
  return {
    ...node,
    [NODE_POLICY_KEYS.autoSelectionEligible]: autoSelectionEligible && !meteredProtected,
    [NODE_POLICY_KEYS.meteredProtected]: meteredProtected,
  }
}

export function validateNodeForSave(node: SingBoxOutbound): string | null {
  if (!node.tag?.trim()) return '请输入节点名称'
  if (!node.type?.trim()) return '节点协议无效'
  if (node.type !== 'wireguard') {
    if (!node.server?.trim()) return '请输入服务器地址'
    if (!Number.isInteger(node.server_port) || (node.server_port ?? 0) < 1 || (node.server_port ?? 0) > 65535) {
      return '端口必须在 1 到 65535 之间'
    }
  }
  return null
}

export function nodePolicyState(node: SingBoxOutbound) {
  const meteredProtected = node[NODE_POLICY_KEYS.meteredProtected] === true
  return {
    meteredProtected,
    autoSelectionEligible: !meteredProtected && node[NODE_POLICY_KEYS.autoSelectionEligible] !== false,
  }
}
