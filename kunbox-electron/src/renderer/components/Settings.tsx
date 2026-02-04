import { useState, useEffect } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import * as Switch from '@radix-ui/react-switch'
import * as Select from '@radix-ui/react-select'
import { ChevronDown, Check, Globe, Shield, Wifi, Gauge, Monitor, RefreshCw, Settings2 } from 'lucide-react'
import type { AppSettings } from '../../shared/types'

type SettingsTab = 'proxy' | 'tun' | 'dns' | 'system'

const TAB_ITEMS = [
  { id: 'proxy' as const, label: '代理', icon: Globe },
  { id: 'tun' as const, label: 'TUN', icon: Shield },
  { id: 'dns' as const, label: 'DNS', icon: Wifi },
  { id: 'system' as const, label: '系统', icon: Monitor },
]

export default function Settings() {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [saving, setSaving] = useState(false)
  const [activeTab, setActiveTab] = useState<SettingsTab>(() => {
    const savedTab = localStorage.getItem('kunbox-settings-tab') as SettingsTab | null
    if (savedTab && ['proxy', 'tun', 'dns', 'system'].includes(savedTab)) {
      localStorage.removeItem('kunbox-settings-tab')
      return savedTab
    }
    return 'proxy'
  })

  useEffect(() => {
    window.api.settings.get().then(setSettings)
  }, [])

  const updateSetting = async <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    if (!settings) return
    const newSettings = { ...settings, [key]: value }
    setSettings(newSettings)
    setSaving(true)
    await window.api.settings.set({ [key]: value })
    if (key === 'theme') {
      localStorage.setItem('kunbox-theme', value as string)
      // Dispatch custom event to trigger theme change in App.tsx
      window.dispatchEvent(new CustomEvent('theme-change'))
    }
    setSaving(false)
  }

  if (!settings) {
    return (
      <div className="h-full flex items-center justify-center">
        <RefreshCw className="w-6 h-6 text-text-faint animate-spin" />
      </div>
    )
  }

  return (
    <div className="h-full flex flex-col px-6 pb-6">
      <div className="flex items-center justify-between mb-8">
        <div className="flex items-center gap-4">
          <div className="p-3 bg-gradient-to-br from-[var(--accent-primary)]/15 to-[var(--accent-primary)]/5 rounded-2xl border border-[var(--accent-primary)]/20">
            <Settings2 className="w-6 h-6 text-[var(--accent-primary)]" />
          </div>
          <div>
            <h2 className="text-2xl font-bold tracking-tight text-[var(--text-primary)]">设置</h2>
            <p className="text-[var(--text-muted)] text-sm">配置代理与系统选项</p>
          </div>
        </div>
        {saving && <span className="text-xs text-[var(--text-faint)]">保存中...</span>}
      </div>

      <div className="flex justify-center mb-8">
        <div className="relative inline-flex p-1.5 bg-[var(--glass-bg)]/60 backdrop-blur-2xl rounded-2xl border border-[var(--glass-border)]">
          {TAB_ITEMS.map((item) => (
            <button
              key={item.id}
              onClick={() => setActiveTab(item.id)}
              className={`relative flex items-center gap-2.5 px-6 py-3 rounded-xl text-sm font-semibold transition-colors duration-300 z-10 ${
                activeTab === item.id
                  ? 'text-white'
                  : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'
              }`}
            >
              <item.icon className={`w-4 h-4 transition-transform duration-300 ${activeTab === item.id ? 'scale-110' : 'scale-100'}`} />
              {item.label}
              {activeTab === item.id && (
                <motion.div
                  layoutId="settings-tab-bg"
                  className="absolute inset-0 bg-gradient-to-br from-[var(--accent-primary)] to-[var(--accent-secondary,var(--accent-primary))] rounded-xl -z-10 shadow-lg shadow-[var(--accent-primary)]/25"
                  transition={{ type: "spring", bounce: 0.15, duration: 0.5 }}
                />
              )}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-hidden">
        <AnimatePresence mode="wait">
          <motion.div
            key={activeTab}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -15 }}
            transition={{ duration: 0.25, ease: [0.25, 0.46, 0.45, 0.94] }}
            className="h-full"
          >
            {activeTab === 'proxy' && (
              <div className="space-y-4">
                <SettingCard>
                  <SettingRow label="HTTP 端口">
                    <NumberInput value={settings.localPort} onChange={(v) => updateSetting('localPort', v)} min={1} max={65535} />
                  </SettingRow>
                  <SettingRow label="SOCKS 端口">
                    <NumberInput value={settings.socksPort} onChange={(v) => updateSetting('socksPort', v)} min={1} max={65535} />
                  </SettingRow>
                  <SettingRow label="允许局域网访问">
                    <Toggle checked={settings.allowLan} onChange={(v) => updateSetting('allowLan', v)} />
                  </SettingRow>
                  <SettingRow label="系统代理" isLast>
                    <Toggle checked={settings.systemProxy} onChange={(v) => updateSetting('systemProxy', v)} />
                  </SettingRow>
                </SettingCard>

                <SettingCard>
                  <SettingRow label="路由模式">
                    <Dropdown
                      value={settings.blockAds ? 'block' : 'normal'}
                      options={[
                        { value: 'normal', label: '正常' },
                        { value: 'block', label: '屏蔽广告' }
                      ]}
                      onChange={(v) => updateSetting('blockAds', v === 'block')}
                    />
                  </SettingRow>
                  <SettingRow label="绕过局域网" isLast>
                    <Toggle checked={settings.bypassLan} onChange={(v) => updateSetting('bypassLan', v)} />
                  </SettingRow>
                </SettingCard>
              </div>
            )}

            {activeTab === 'tun' && (
              <div className="space-y-4">
                <SettingCard>
                  <SettingRow label="启用 TUN 模式">
                    <Toggle checked={settings.tunEnabled} onChange={(v) => updateSetting('tunEnabled', v)} />
                  </SettingRow>
                  <SettingRow label="网络栈" isLast>
                    <Dropdown
                      value={settings.tunStack}
                      options={[
                        { value: 'system', label: 'System' },
                        { value: 'gvisor', label: 'gVisor' },
                        { value: 'mixed', label: 'Mixed' }
                      ]}
                      onChange={(v) => updateSetting('tunStack', v as AppSettings['tunStack'])}
                    />
                  </SettingRow>
                </SettingCard>

                <div className="glass-card rounded-2xl p-5">
                  <p className="text-sm text-[var(--text-muted)]">
                    TUN 模式可接管系统全部流量，需要管理员权限。建议使用 gVisor 或 Mixed 网络栈以获得更好的兼容性。
                  </p>
                </div>
              </div>
            )}

            {activeTab === 'dns' && (
              <div className="space-y-4">
                <SettingCard>
                  <SettingRow label="本地 DNS">
                    <TextInput value={settings.localDns} onChange={(v) => updateSetting('localDns', v)} placeholder="223.5.5.5" />
                  </SettingRow>
                  <SettingRow label="远程 DNS">
                    <TextInput value={settings.remoteDns} onChange={(v) => updateSetting('remoteDns', v)} placeholder="https://dns.google/dns-query" />
                  </SettingRow>
                  <SettingRow label="启用 FakeDNS" isLast>
                    <Toggle checked={settings.fakeDns} onChange={(v) => updateSetting('fakeDns', v)} />
                  </SettingRow>
                </SettingCard>

                <SettingCard>
                  <SettingRow label="测试 URL">
                    <TextInput value={settings.latencyTestUrl} onChange={(v) => updateSetting('latencyTestUrl', v)} placeholder="https://www.gstatic.com/generate_204" />
                  </SettingRow>
                  <SettingRow label="超时时间" isLast>
                    <NumberInput value={settings.latencyTestTimeout} onChange={(v) => updateSetting('latencyTestTimeout', v)} min={1000} max={30000} step={1000} />
                  </SettingRow>
                </SettingCard>
              </div>
            )}

            {activeTab === 'system' && (
              <div className="space-y-4">
                <SettingCard>
                  <SettingRow label="主题">
                    <Dropdown
                      value={settings.theme}
                      options={[
                        { value: 'dark', label: '深色' },
                        { value: 'light', label: '浅色' },
                        { value: 'system', label: '跟随系统' }
                      ]}
                      onChange={(v) => updateSetting('theme', v as AppSettings['theme'])}
                      isTheme
                    />
                  </SettingRow>
                  <SettingRow label="启动时自动连接">
                    <Toggle checked={settings.autoConnect} onChange={(v) => updateSetting('autoConnect', v)} />
                  </SettingRow>
                  <SettingRow label="最小化到托盘">
                    <Toggle checked={settings.minimizeToTray} onChange={(v) => updateSetting('minimizeToTray', v)} />
                  </SettingRow>
                  <SettingRow label="开机自启" isLast>
                    <Toggle checked={settings.startWithWindows} onChange={(v) => updateSetting('startWithWindows', v)} />
                  </SettingRow>
                </SettingCard>
              </div>
            )}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  )
}

function SettingCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="glass-card rounded-2xl overflow-hidden divide-y divide-[var(--border-secondary)]">
      {children}
    </div>
  )
}

function SettingRow({ label, children, isLast }: { label: string; children: React.ReactNode; isLast?: boolean }) {
  return (
    <div className={`flex items-center justify-between px-6 py-4 ${isLast ? '' : ''}`}>
      <span className="text-sm font-medium text-[var(--text-secondary)]">{label}</span>
      {children}
    </div>
  )
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <Switch.Root
      checked={checked}
      onCheckedChange={onChange}
      className="w-12 h-7 rounded-full bg-[var(--bg-tertiary)] data-[state=checked]:bg-[var(--accent-primary)] transition-colors"
    >
      <Switch.Thumb className="block w-5 h-5 bg-white rounded-full transition-transform translate-x-1 data-[state=checked]:translate-x-6" />
    </Switch.Root>
  )
}

function NumberInput({ value, onChange, min, max, step = 1 }: { value: number; onChange: (v: number) => void; min?: number; max?: number; step?: number }) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={(e) => onChange(Number(e.target.value))}
      className="glass-input w-28 !py-2 rounded-xl text-center"
    />
  )
}

function TextInput({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder?: string }) {
  return (
    <input
      type="text"
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      className="glass-input w-64 !py-2 rounded-xl"
    />
  )
}

function Dropdown({ value, options, onChange, isTheme }: { value: string; options: { value: string; label: string }[]; onChange: (v: string) => void; isTheme?: boolean }) {
  const handleChange = (v: string, e?: React.MouseEvent) => {
    if (isTheme && e) {
      document.documentElement.style.setProperty('--x', e.clientX + 'px')
      document.documentElement.style.setProperty('--y', e.clientY + 'px')
    }
    onChange(v)
  }

  return (
    <Select.Root value={value} onValueChange={handleChange}>
      <Select.Trigger className="glass-select inline-flex items-center justify-between gap-2 w-36 !py-2 rounded-xl">
        <Select.Value />
        <Select.Icon>
          <ChevronDown className="w-4 h-4 text-[var(--text-faint)]" />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Content className="glass-card shadow-soft-lg overflow-hidden z-50 rounded-xl">
          <Select.Viewport className="p-1">
            {options.map((opt) => (
              <Select.Item
                key={opt.value}
                value={opt.value}
                className="flex items-center justify-between gap-2 px-4 py-2.5 text-sm text-[var(--text-primary)] rounded-lg cursor-pointer outline-none hover:bg-[var(--bg-hover)] data-[highlighted]:bg-[var(--bg-hover)]"
                onClick={(e) => isTheme && handleChange(opt.value, e)}
              >
                <Select.ItemText>{opt.label}</Select.ItemText>
                <Select.ItemIndicator>
                  <Check className="w-4 h-4 text-[var(--accent-primary)]" />
                </Select.ItemIndicator>
              </Select.Item>
            ))}
          </Select.Viewport>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  )
}
