import { shouldCommitLatencyResult } from './nodesStore.ts'

if (!shouldCommitLatencyResult(10, 10)) throw new Error('当前测速代际被错误丢弃')
if (shouldCommitLatencyResult(10, 11)) throw new Error('旧测速结果没有被丢弃')
