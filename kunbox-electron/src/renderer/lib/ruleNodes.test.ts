import { getMissingRuleNodeWarning, getRuleNodeValue, resolveRuleNode } from './ruleNodes.ts'

const nodes = [
  { tag: 'RU-1', sourceProfileId: 'profile-a', sourceProfileName: 'Alpha' },
  { tag: 'RU-1', sourceProfileId: 'profile-b', sourceProfileName: 'Beta' },
]

if (getRuleNodeValue('profile-a::RU-2', nodes) !== '') throw new Error('失效节点应显示为空')
if (getRuleNodeValue(undefined, nodes) !== '') throw new Error('未选择节点应显示为空')
if (getRuleNodeValue('deleted-profile::RU-1', nodes) !== '') throw new Error('不能使用其他配置的同名节点')
if (getRuleNodeValue('profile-b::RU-1', nodes) !== 'profile-b::RU-1') throw new Error('有效节点选择应保留')
if (resolveRuleNode('profile-b::RU-1', nodes)?.sourceProfileName !== 'Beta') throw new Error('显示了错误配置的节点')
if (getRuleNodeValue('RU-1', nodes) !== 'profile-a::RU-1') throw new Error('节点名称引用未正确显示')

const rules = [
  { label: 'vk.ru', enabled: true, outboundMode: 'node', outboundValue: 'profile-a::RU-2' },
  { label: 'edgecenter.ru', enabled: true, outboundMode: 'node' },
  { label: 'valid', enabled: true, outboundMode: 'node', outboundValue: 'profile-b::RU-1' },
  { label: 'disabled', enabled: false, outboundMode: 'node', outboundValue: 'missing' },
  { label: 'direct', enabled: true, outboundMode: 'direct' },
]
const warning = getMissingRuleNodeWarning(rules, nodes)
if (!warning?.includes('vk.ru、edgecenter.ru') || !warning.includes('当前选中的代理')) {
  throw new Error('未提示失效规则及默认代理行为')
}
if (warning.includes('valid') || warning.includes('disabled') || warning.includes('direct')) {
  throw new Error('正常规则或未启用规则不应提示')
}
if (getMissingRuleNodeWarning([rules[2]], nodes) !== null) throw new Error('有效节点不应提示')
