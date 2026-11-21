export function isNewerRelease(local: string | undefined, remote: string): boolean {
  const parse = (version: string) => {
    const match = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(version)
    return match ? match.slice(1).map(BigInt) : null
  }
  const candidate = parse(remote)
  if (!candidate) return false
  if (!local || /^1\.0\.0-[0-9a-f]{12}$/.test(local)) return true
  const current = parse(local)
  if (!current) return false
  for (let index = 0; index < candidate.length; index += 1) {
    if (candidate[index] !== current[index]) return candidate[index] > current[index]
  }
  return false
}
