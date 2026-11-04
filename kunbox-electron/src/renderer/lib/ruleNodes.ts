import type { NodeWithProfile } from '../../shared/types'

type RuleNode = Pick<NodeWithProfile, 'tag' | 'sourceProfileId' | 'sourceProfileName'>

export function resolveRuleNode(value: string | undefined, nodes: readonly RuleNode[]) {
  if (!value) return undefined
  return nodes.find((node) => node.tag && (
    `${node.sourceProfileId}::${node.tag}` === value || node.tag === value
  ))
}

export function getRuleNodeValue(value: string | undefined, nodes: readonly RuleNode[]) {
  const node = resolveRuleNode(value, nodes)
  return node ? `${node.sourceProfileId}::${node.tag}` : ''
}

interface NodeRule {
  label: string
  enabled: boolean
  outboundMode: string
  outboundValue?: string
}

export function getMissingRuleNodeWarning(rules: readonly NodeRule[], nodes: readonly RuleNode[]) {
  const missing = rules.filter((rule) => rule.enabled && rule.outboundMode === 'node'
    && !resolveRuleNode(rule.outboundValue, nodes))
  return missing.length
    ? `以下分流规则未选择有效节点：${missing.map((rule) => rule.label).join('、')}。默认走当前选中的代理。`
    : null
}
