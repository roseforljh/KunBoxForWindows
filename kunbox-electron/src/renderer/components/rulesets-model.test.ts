import { buildHubRuleSets, isHubFormatAvailable, sanitizeBuiltInHubRule } from './rulesets-model.ts'

const hubs = buildHubRuleSets([
  { type: 'blob', path: 'geosite-cn.srs' },
  { type: 'blob', path: 'geosite-cn.json' },
  { type: 'blob', path: 'geoip-cn.srs' },
  { type: 'tree', path: 'ignored.srs' },
])

const geosite = hubs.find((hub) => hub.name === 'geosite-cn')
if (!geosite?.binaryUrl?.endsWith('/sing-geosite/rule-set/geosite-cn.srs')) throw new Error('Binary 地址生成错误')
if (!geosite.sourceUrl?.endsWith('/sing-geosite/rule-set/geosite-cn.json')) throw new Error('Source 地址生成错误')
if (!isHubFormatAvailable(geosite, 'binary') || !isHubFormatAvailable(geosite, 'source')) throw new Error('双格式识别错误')

const geoip = hubs.find((hub) => hub.name === 'geoip-cn')
if (!geoip || isHubFormatAvailable(geoip, 'source')) throw new Error('仅 Binary 规则集错误显示 Source')

const builtIn = sanitizeBuiltInHubRule({
  name: 'geosite-cn',
  tags: ['Official'],
  sourceUrl: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.json',
  binaryUrl: 'https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs',
})
if (builtIn.sourceUrl) throw new Error('官方失效 Source 地址未被清理')
