import { useEffect, useState } from 'react'
import type { AppSettings } from '@shared/types'

export function RoutingModeNotice() {
  const [mode, setMode] = useState<AppSettings['routingMode'] | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let active = true
    window.api.settings.get().then(settings => {
      if (active) setMode(settings.routingMode)
    }).catch(() => {
      if (active) setError(true)
    })
    return () => { active = false }
  }, [])

  if (!error && (mode === null || mode === 'rule')) return null

  return (
    <div role="alert" className="mb-4 rounded-xl border border-amber-500/30 bg-amber-500/15 p-4 text-sm text-amber-500">
      {error ? '无法读取路由模式，请到设置页确认当前是否使用规则模式。' : (
        <>
          当前保存的路由模式为「{mode === 'global-proxy' ? '全局代理' : '全局直连'}」，此模式会忽略域名分流和规则集的指定节点。
          请在「设置 → 代理」切换为「规则模式」，然后重启 VPN。TUN 负责接管流量，不会自动开启规则模式。
        </>
      )}
    </div>
  )
}
