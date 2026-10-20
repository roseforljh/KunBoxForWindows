import assert from 'node:assert/strict'
import {
  applyNodePolicies,
  DEFAULT_MULTIPLEX,
  enableMultiplex,
  makeNodeReference,
  NODE_RUNTIME_KEYS,
  nodePolicyState,
  setMultiplexPolicy,
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

const defaultMux = JSON.parse(JSON.stringify(enableMultiplex()))
assert.deepEqual(defaultMux, DEFAULT_MULTIPLEX, '开启多路复用未写入默认值')
if ('max_streams' in defaultMux) throw new Error('均衡默认包含互斥的流数限制')
const customMux = enableMultiplex({ protocol: 'smux', max_connections: 8, min_streams: 2, padding: true })
if (customMux.protocol !== 'smux' || customMux.max_connections !== 8 || customMux.min_streams !== 2 || !customMux.padding) {
  throw new Error('开启多路复用覆盖了已有自定义参数')
}
const streamMux = enableMultiplex({ max_streams: 12 })
if (streamMux.max_connections !== undefined || streamMux.min_streams !== undefined || streamMux.max_streams !== 12) {
  throw new Error('高级流数模式被注入冲突默认值')
}
for (const mux of [defaultMux, customMux, streamMux, setMultiplexPolicy(customMux, true), setMultiplexPolicy(streamMux, false)]) {
  if (validateNodeForSave({ ...validNode, type: 'vmess', multiplex: mux })) throw new Error('有效多路复用设置被拒绝')
}
for (const mux of [
  { ...DEFAULT_MULTIPLEX, max_streams: 16 },
  { ...DEFAULT_MULTIPLEX, max_connections: -1 },
  { ...DEFAULT_MULTIPLEX, min_streams: 1.5 },
  { ...DEFAULT_MULTIPLEX, max_connections: Number.NaN },
  { ...DEFAULT_MULTIPLEX, protocol: 'invalid' },
]) {
  if (!validateNodeForSave({ ...validNode, multiplex: mux })) throw new Error('非法多路复用设置未被拒绝')
}
if (!validateNodeForSave({ ...validNode, type: 'vless', transport: { type: 'xhttp' }, multiplex: defaultMux })) {
  throw new Error('XHTTP 上无效的 sing-box 多路复用未被拒绝')
}
const hy2 = {
  ...validNode, type: 'hysteria2', server_port: undefined, server_ports: ['20000:30000', 443],
  disable_chrome_parrot: true, bbr_profile: 'conservative' as const,
}
if (validateNodeForSave(hy2)) throw new Error('端口跳跃节点不能保存 HY2 设置')
for (const ports of [['0'], ['65536'], ['30000:20000'], ['abc'], ['1:2:3'], ['0x1bb'], ['4e3']]) {
  if (!validateNodeForSave({ ...hy2, server_ports: ports })) throw new Error('非法端口跳跃未被拒绝')
}
const hy2Draft = toEditableNode(hy2)
if (hy2Draft.disable_chrome_parrot !== true || hy2Draft.bbr_profile !== 'conservative') {
  throw new Error('HY2 新选项在编辑时丢失')
}
for (const timing of [
  { hop_interval: '30s', hop_interval_max: '1m' },
  { hop_interval: '1m30s' },
  { hop_interval: '500ms', hop_interval_max: '1.5s' },
]) {
  if (validateNodeForSave({ ...hy2, ...timing })) throw new Error('有效跳跃间隔被拒绝')
}
for (const timing of [
  { hop_interval: '0s' }, { hop_interval: '-1s' }, { hop_interval: '30' },
  { hop_interval: 'invalid' }, { hop_interval_max: '20s' },
  { hop_interval: '30s', hop_interval_max: '10s' },
]) {
  if (!validateNodeForSave({ ...hy2, ...timing })) throw new Error('无效跳跃间隔未被拒绝')
}
const automaticHy2 = toEditableNode({ ...validNode, type: 'hysteria2' })
if ('disable_chrome_parrot' in automaticHy2 || 'bbr_profile' in automaticHy2) {
  throw new Error('未修改 HY2 选项时不应注入新版本专用字段')
}
