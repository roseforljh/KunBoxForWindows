import { applyNodePolicies, makeNodeReference, nodePolicyState, validateNodeForSave } from './node-editor'

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
