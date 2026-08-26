export type RuleSetFormat = 'binary' | 'source'

export interface HubRuleSet {
  name: string
  tags: string[]
  sourceUrl?: string
  binaryUrl?: string
}

export interface HubTreeEntry {
  type: string
  path: string
}

function repoForRuleSet(name: string) {
  return name.startsWith('geoip-') ? 'sing-geoip' : 'sing-geosite'
}

function officialRawUrl(name: string, extension: string) {
  return `https://raw.githubusercontent.com/SagerNet/${repoForRuleSet(name)}/rule-set/${name}.${extension}`
}

export function buildHubRuleSets(entries: HubTreeEntry[]): HubRuleSet[] {
  const byName = new Map<string, HubRuleSet>()
  for (const entry of entries) {
    if (entry.type !== 'blob' || !entry.path.endsWith('.srs') && !entry.path.endsWith('.json')) continue
    const extension = entry.path.endsWith('.srs') ? 'srs' : 'json'
    const name = entry.path.slice(0, -extension.length - 1).split('/').pop() ?? ''
    if (!name) continue
    const current = byName.get(name) ?? {
      name,
      tags: ['Official', name.startsWith('geoip-') ? 'geoip' : 'geosite'],
    }
    if (extension === 'srs') current.binaryUrl = officialRawUrl(name, extension)
    else current.sourceUrl = officialRawUrl(name, extension)
    byName.set(name, current)
  }
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name))
}

export function isHubFormatAvailable(hub: HubRuleSet, format: RuleSetFormat) {
  return format === 'binary' ? Boolean(hub.binaryUrl) : Boolean(hub.sourceUrl)
}

export function sanitizeBuiltInHubRule(hub: HubRuleSet): HubRuleSet {
  if (
    hub.sourceUrl &&
    /(?:sing-geosite|sing-geoip)\/rule-set\/[^/]+\.json$/i.test(hub.sourceUrl)
  ) {
    return { ...hub, sourceUrl: undefined }
  }
  return hub
}
