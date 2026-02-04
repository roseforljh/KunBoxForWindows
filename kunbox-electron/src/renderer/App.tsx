import { useState, useEffect, useCallback, forwardRef, useImperativeHandle, useRef } from 'react'
import { motion, AnimatePresence, useAnimation } from 'framer-motion'
import { Minus, Square, X, PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { useConnectionStore } from './stores/connectionStore'
import Dashboard from './components/Dashboard'
import Nodes from './components/Nodes'
import Profiles from './components/Profiles'
import SettingsPage from './components/Settings'
import Logs from './components/Logs'
import RuleSets from './components/RuleSets'
import DomainRules from './components/DomainRules'
import ProcessRules from './components/ProcessRules'
import logoImg from './assets/logo.png'

type Page = 'dashboard' | 'nodes' | 'profiles' | 'settings' | 'logs' | 'rulesets' | 'domainrules' | 'processrules'
type Theme = 'dark' | 'light' | 'system'

interface IconHandle {
  startAnimation: () => void
  stopAnimation: () => void
}

interface AnimatedIconProps {
  size?: number
  className?: string
}

const defaultTransition = {
  type: "spring" as const,
  stiffness: 160,
  damping: 17,
  mass: 1,
}

const GaugeIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <motion.path
          animate={controls}
          d="m12 14 4-4"
          transition={defaultTransition}
          variants={{
            animate: { translateX: 0.5, translateY: 3, rotate: 72 },
            normal: { translateX: 0, rotate: 0, translateY: 0 },
          }}
        />
        <path d="M3.34 19a10 10 0 1 1 17.32 0" />
      </svg>
    </div>
  )
})

const GlobeIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="10" />
        <motion.path
          animate={controls}
          d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"
          transition={defaultTransition}
          variants={{
            animate: { rotate: 20 },
            normal: { rotate: 0 },
          }}
          style={{ transformOrigin: "center" }}
        />
        <path d="M2 12h20" />
      </svg>
    </div>
  )
})

const FolderIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <motion.path
          animate={controls}
          d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"
          transition={defaultTransition}
          variants={{
            animate: { y: -2 },
            normal: { y: 0 },
          }}
        />
      </svg>
    </div>
  )
})

const ShieldIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <motion.path
          animate={controls}
          d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"
          transition={defaultTransition}
          variants={{
            animate: { scale: 1.1 },
            normal: { scale: 1 },
          }}
          style={{ transformOrigin: "center" }}
        />
      </svg>
    </div>
  )
})

const RouteIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="6" cy="19" r="3" />
        <motion.path
          animate={controls}
          d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15"
          transition={defaultTransition}
          variants={{
            animate: { pathLength: 1, opacity: 1 },
            normal: { pathLength: 0.8, opacity: 0.8 },
          }}
        />
        <circle cx="18" cy="5" r="3" />
      </svg>
    </div>
  )
})

const CpuIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <motion.rect
          animate={controls}
          x="4" y="4" width="16" height="16" rx="2"
          transition={defaultTransition}
          variants={{
            animate: { scale: 1.05 },
            normal: { scale: 1 },
          }}
          style={{ transformOrigin: "center" }}
        />
        <rect x="9" y="9" width="6" height="6" />
        <path d="M15 2v2" />
        <path d="M15 20v2" />
        <path d="M2 15h2" />
        <path d="M2 9h2" />
        <path d="M20 15h2" />
        <path d="M20 9h2" />
        <path d="M9 2v2" />
        <path d="M9 20v2" />
      </svg>
    </div>
  )
})

const SettingsIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <motion.g
          animate={controls}
          transition={{ ...defaultTransition, duration: 0.5 }}
          variants={{
            animate: { rotate: 90 },
            normal: { rotate: 0 },
          }}
          style={{ transformOrigin: "center" }}
        >
          <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
          <circle cx="12" cy="12" r="3" />
        </motion.g>
      </svg>
    </div>
  )
})

const FileTextIcon = forwardRef<IconHandle, AnimatedIconProps>(({ size = 20, className }, ref) => {
  const controls = useAnimation()
  const isControlledRef = useRef(false)

  useImperativeHandle(ref, () => {
    isControlledRef.current = true
    return {
      startAnimation: () => controls.start("animate"),
      stopAnimation: () => controls.start("normal"),
    }
  })

  return (
    <div className={className}>
      <svg fill="none" height={size} width={size} viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
        <path d="M14 2v4a2 2 0 0 0 2 2h4" />
        <motion.g
          animate={controls}
          transition={defaultTransition}
          variants={{
            animate: { opacity: 1, x: 0 },
            normal: { opacity: 0.7, x: -2 },
          }}
        >
          <path d="M10 9H8" />
          <path d="M16 13H8" />
          <path d="M16 17H8" />
        </motion.g>
      </svg>
    </div>
  )
})

const navItems = [
  { id: 'dashboard' as Page, Icon: GaugeIcon, label: '仪表盘' },
  { id: 'nodes' as Page, Icon: GlobeIcon, label: '节点' },
  { id: 'profiles' as Page, Icon: FolderIcon, label: '订阅' },
  { id: 'rulesets' as Page, Icon: ShieldIcon, label: '规则集' },
  { id: 'domainrules' as Page, Icon: RouteIcon, label: '域名分流' },
  { id: 'processrules' as Page, Icon: CpuIcon, label: '进程分流' }
]

const footerItems = [
  { id: 'settings' as Page, Icon: SettingsIcon, label: '设置' },
  { id: 'logs' as Page, Icon: FileTextIcon, label: '日志' }
]

function BokehBackground() {
  return (
    <div className="bokeh-bg">
      <div className="bokeh-blob bokeh-blob-1" />
      <div className="bokeh-blob bokeh-blob-2" />
    </div>
  )
}

function TopBar() {
  return (
    <div
      className="glass-topbar h-12 flex items-center justify-between px-4"
      style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}
    >
      <div className="flex items-center gap-3 ml-2" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
        <span className="text-lg font-semibold" style={{ color: 'var(--text-primary)' }}>
          KunBox
        </span>
        <span 
          className="text-xs px-2 py-0.5 rounded"
          style={{ color: 'var(--text-muted)', background: 'var(--bg-hover)' }}
        >
          v1.0.0
        </span>
      </div>

      <div className="flex items-center gap-2" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
        <button
          onClick={() => window.api.window.minimize()}
          className="w-8 h-8 flex items-center justify-center rounded-lg transition-all duration-200 hover:bg-[var(--bg-hover)]"
          style={{ color: 'var(--text-muted)' }}
        >
          <Minus className="w-4 h-4" />
        </button>
        <button
          onClick={() => window.api.window.maximize()}
          className="w-8 h-8 flex items-center justify-center rounded-lg transition-all duration-200 hover:bg-[var(--bg-hover)]"
          style={{ color: 'var(--text-muted)' }}
        >
          <Square className="w-3 h-3" />
        </button>
        <button
          onClick={() => window.api.window.close()}
          className="w-8 h-8 flex items-center justify-center rounded-lg transition-all duration-200 hover:bg-[var(--error)] hover:text-white"
          style={{ color: 'var(--text-muted)' }}
        >
          <X className="w-4 h-4" />
        </button>
      </div>
    </div>
  )
}

function useTheme() {
  const [theme, setTheme] = useState<Theme>(() => {
    const cached = localStorage.getItem('kunbox-theme') as Theme | null
    return cached || 'dark'
  })

  useEffect(() => {
    window.api.settings.get().then((settings) => {
      if (settings?.theme) {
        setTheme(settings.theme)
        localStorage.setItem('kunbox-theme', settings.theme)
      }
    })
  }, [])

  // Listen for localStorage changes (from Settings page)
  useEffect(() => {
    const handleStorageChange = () => {
      const newTheme = localStorage.getItem('kunbox-theme') as Theme | null
      if (newTheme && newTheme !== theme) {
        setTheme(newTheme)
      }
    }

    // Custom event for same-window localStorage updates
    window.addEventListener('theme-change', handleStorageChange)
    // Storage event for cross-window updates
    window.addEventListener('storage', handleStorageChange)

    return () => {
      window.removeEventListener('theme-change', handleStorageChange)
      window.removeEventListener('storage', handleStorageChange)
    }
  }, [theme])

  useEffect(() => {
    const root = document.documentElement

    const applyTheme = (resolvedTheme: 'dark' | 'light') => {
      root.classList.remove('dark', 'light')
      root.classList.add(resolvedTheme)
    }

    if (theme === 'system') {
      const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
      applyTheme(mediaQuery.matches ? 'dark' : 'light')

      const handler = (e: MediaQueryListEvent) => applyTheme(e.matches ? 'dark' : 'light')
      mediaQuery.addEventListener('change', handler)
      return () => mediaQuery.removeEventListener('change', handler)
    } else {
      applyTheme(theme)
    }
  }, [theme])

  return { theme, setTheme }
}

export default function App() {
  const [currentPage, setCurrentPage] = useState<Page>('dashboard')
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const { setState, setTraffic } = useConnectionStore()
  const { setTheme } = useTheme()

  useEffect(() => {
    const unsubState = window.api.singbox.onStateChange((state) => {
      setState(state)
    })

    const unsubTraffic = window.api.singbox.onTraffic((stats) => {
      setTraffic(stats)
    })

    return () => {
      unsubState()
      unsubTraffic()
    }
  }, [setState, setTraffic])

  const handleNavigate = useCallback((page: string, tab?: string) => {
    if (tab) {
      localStorage.setItem('kunbox-settings-tab', tab)
    }
    setCurrentPage(page as Page)
  }, [])

  const renderPage = () => {
    switch (currentPage) {
      case 'dashboard': return <Dashboard />
      case 'nodes': return <Nodes />
      case 'profiles': return <Profiles />
      case 'settings': return <SettingsPage />
      case 'logs': return <Logs />
      case 'rulesets': return <RuleSets />
      case 'domainrules': return <DomainRules />
      case 'processrules': return <ProcessRules onNavigate={handleNavigate} />
    }
  }

  return (
    <div className="flex flex-col h-screen relative" style={{ backgroundColor: 'var(--bg-primary)' }}>
      <BokehBackground />
      <TopBar />

      <div className="flex flex-1 overflow-hidden">
        <aside className={`glass-sidebar flex-shrink-0 flex flex-col overflow-hidden transition-[width] duration-300 ease-in-out ${sidebarCollapsed ? 'w-[72px]' : 'w-64'}`}>
          <div className="flex items-center gap-3 p-4 min-h-[68px] overflow-hidden">
            <img 
              src={logoImg} 
              alt="KunBox" 
              className="w-8 h-8 flex-shrink-0"
            />
            <div className={`transition-all duration-300 overflow-hidden ${sidebarCollapsed ? 'w-0 opacity-0' : 'w-auto opacity-100'}`}>
              <div className="text-lg font-bold whitespace-nowrap" style={{ color: 'var(--text-primary)' }}>KunBox</div>
              <div className="text-xs whitespace-nowrap" style={{ color: 'var(--text-muted)' }}>Proxy Client</div>
            </div>
          </div>

          <div className={`border-b opacity-50 transition-all duration-300 ${sidebarCollapsed ? 'mx-2' : 'mx-4'}`} style={{ borderColor: 'var(--glass-border)' }} />

          <nav className="flex-1 p-3 space-y-1 overflow-x-hidden overflow-y-auto">
            {navItems.map((item) => (
              <NavButton
                key={item.id}
                Icon={item.Icon}
                label={item.label}
                isActive={currentPage === item.id}
                onClick={() => setCurrentPage(item.id)}
                collapsed={sidebarCollapsed}
              />
            ))}
          </nav>

          <div className={`border-t opacity-50 transition-all duration-300 ${sidebarCollapsed ? 'mx-2' : 'mx-4'}`} style={{ borderColor: 'var(--glass-border)' }} />

          <div className="p-3 space-y-1">
            {footerItems.map((item) => (
              <NavButton
                key={item.id}
                Icon={item.Icon}
                label={item.label}
                isActive={currentPage === item.id}
                onClick={() => setCurrentPage(item.id)}
                collapsed={sidebarCollapsed}
              />
            ))}
          </div>

          <div className={`border-t opacity-50 transition-all duration-300 ${sidebarCollapsed ? 'mx-2' : 'mx-4'}`} style={{ borderColor: 'var(--glass-border)' }} />

          <div className="p-3 overflow-hidden flex justify-center">
            <button
              className="w-8 h-8 flex items-center justify-center rounded-lg transition-all duration-300 hover:bg-[var(--bg-hover)]"
              onClick={() => setSidebarCollapsed(!sidebarCollapsed)}
              style={{ color: 'var(--text-muted)' }}
            >
              {sidebarCollapsed ? <PanelLeftOpen size={20} /> : <PanelLeftClose size={20} />}
            </button>
          </div>
        </aside>

        <main className="flex-1 overflow-hidden pt-6 flex flex-col">
          <AnimatePresence mode="wait">
            <motion.div
              key={currentPage}
              initial={{ opacity: 0, y: 18, scale: 0.99 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: -12, scale: 0.99 }}
              transition={{ duration: 0.3, ease: [0.25, 0.46, 0.45, 0.94] }}
              className="flex-1 min-h-0 flex flex-col overflow-auto"
            >
              {renderPage()}
            </motion.div>
          </AnimatePresence>
        </main>
      </div>
    </div>
  )
}

interface NavButtonProps {
  Icon: React.ForwardRefExoticComponent<AnimatedIconProps & React.RefAttributes<IconHandle>>
  label: string
  isActive: boolean
  onClick: () => void
  collapsed?: boolean
}

function NavButton({ Icon, label, isActive, onClick, collapsed = false }: NavButtonProps) {
  const iconRef = useRef<IconHandle>(null)

  const handleMouseEnter = useCallback(() => {
    iconRef.current?.startAnimation()
  }, [])

  const handleMouseLeave = useCallback(() => {
    if (!isActive) {
      iconRef.current?.stopAnimation()
    }
  }, [isActive])

  useEffect(() => {
    if (isActive) {
      iconRef.current?.startAnimation()
    } else {
      iconRef.current?.stopAnimation()
    }
  }, [isActive])

  return (
    <button
      onClick={onClick}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      className={`nav-item w-full flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium transition-all duration-300 ${
        isActive ? 'nav-item-active' : 'nav-item-inactive'
      }`}
    >
      <span 
        className={`flex-shrink-0 flex items-center justify-center w-5 h-5 ${
          isActive ? 'drop-shadow-[var(--glow-primary)]' : ''
        }`}
      >
        <Icon ref={iconRef} size={20} />
      </span>
      <span className={`whitespace-nowrap transition-all duration-300 overflow-hidden ${collapsed ? 'w-0 opacity-0' : 'w-auto opacity-100'}`}>{label}</span>
    </button>
  )
}
