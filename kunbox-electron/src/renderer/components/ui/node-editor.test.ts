import {
  applyNodePolicies,
  makeNodeReference,
  NODE_RUNTIME_KEYS,
  nodePolicyState,
  toEditableNode,
  validateNodeForSave,
} from './node-editor.ts'

const validNode = { tag: '节点 A', type: 'socks', server: '127.0.0.1', server_port: 1080 }
if (validateNodeForSave(validNode) !== null) throw new Error('有效节点被错误拒绝')
if (!validateNodeForSave({ ...validNode, server_port: 0 })) throw new Error('非法端口未被拒绝')

const protectedNode = applyNodePolicies(validNode, true, true)
const protectedPolicy = nodePolicyState(protectedNode)
if (!protectedPolicy.meteredProtected || protectedPolicy.autoSelectionEligible) {
  throw new Error('高价计费保护与自动探测策略未保持互斥')
}

const reference = makeNodeReference({ sourceProfileId: 'profile-a', tag: '节点 A' })
if (reference !== 'profile-a::节点 A') throw new Error(`前置代理引用错误: ${reference}`)

const editableNode = toEditableNode({
  ...validNode,
  uuid: 'test-uuid',
  tls: { enabled: true, server_name: 'example.com' },
  custom_protocol_field: { enabled: true },
  x_kunbox_auto_selection_eligible: true,
  latencyMs: 123,
  latencyStatus: 'success',
  healthStatus: 'healthy',
  isTimeout: false,
  isTesting: true,
  sourceProfileId: 'profile-a',
  sourceProfileName: 'Profile A',
})
for (const key of NODE_RUNTIME_KEYS) {
  if (key in editableNode) throw new Error(`编辑草稿残留运行态字段: ${key}`)
}
if (editableNode.uuid !== 'test-uuid') throw new Error('编辑草稿丢失 UUID')
if (editableNode.tls?.server_name !== 'example.com') throw new Error('编辑草稿丢失 TLS 配置')
if (editableNode.x_kunbox_auto_selection_eligible !== true) throw new Error('编辑草稿丢失长期策略字段')
if ((editableNode.custom_protocol_field as { enabled?: boolean })?.enabled !== true) {
  throw new Error('编辑草稿丢失协议扩展字段')
}
