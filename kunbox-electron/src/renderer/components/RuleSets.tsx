import { useState, useEffect, useMemo, useCallback, useRef } from 'react'
import { Reorder } from 'framer-motion'
import * as Switch from '@radix-ui/react-switch'
import {
  Shield,
  Plus,
  Globe,
  Zap,
  Ban,
  Edit2,
  Trash2,
  Download,
  RefreshCw,
  Search,
  X,
  Server,
  FileText,
  GripVertical,
  Loader2,
  CheckCircle,
  AlertCircle,
  ExternalLink,
  Check
} from 'lucide-react'
import { Modal, ModalButton } from './ui/Modal'
import { AppSelect } from './ui/Select'
import { useNodesStore } from '../stores/nodesStore'
import { useConnectionStore } from '../stores/connectionStore'
import {
  buildHubRuleSets,
  isHubFormatAvailable,
  sanitizeBuiltInHubRule,
  type HubRuleSet,
  type HubTreeEntry,
} from './rulesets-model'
import { useToast } from './ui/Toast'
import { useProfiles } from '../lib/useProfiles'
import { RoutingModeNotice } from './RoutingModeNotice'

const fastTransition = { duration: 0.15, ease: [0.4, 0, 0.2, 1] as const }

interface RuleSetItem {
  id: string
  tag: string
  name: string
  url: string
  type: 'remote' | 'local'
  format: 'binary' | 'source'
  outboundMode: 'direct' | 'proxy' | 'block' | 'node' | 'profile'
  outboundValue?: string
  enabled: boolean
  isBuiltIn: boolean
}

const RULESET_TYPE_OPTIONS = [
  { value: 'remote', label: '远程' },
  { value: 'local', label: '本地' },
] as const

const RULESET_FORMAT_OPTIONS = [
  { value: 'binary', label: '二进制（SRS）' },
  { value: 'source', label: '源码（JSON）' },
] as const

const RULESET_OUTBOUND_OPTIONS = [
  { value: 'direct', label: '直连，不经过代理' },
  { value: 'proxy', label: '代理，通过当前代理服务器' },
  { value: 'block', label: '拦截，阻止连接' },
  { value: 'node', label: '节点，指定特定节点' },
  { value: 'profile', label: '配置，指定特定配置' },
] as const

const defaultRuleSets: RuleSetItem[] = [
  {
    id: '1',
    tag: 'geosite-cn',
    name: '中国网站',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'direct',
    enabled: true,
    isBuiltIn: true
  },
  {
    id: '2',
    tag: 'geoip-cn',
    name: '中国 IP',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'direct',
    enabled: true,
    isBuiltIn: true
  },
  {
    id: '4',
    tag: 'geosite-category-ads-all',
    name: '广告拦截',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'block',
    enabled: false,
    isBuiltIn: true
  },
  {
    id: '5',
    tag: 'geosite-private',
    name: '私有地址',
    url: 'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-private.srs',
    type: 'remote',
    format: 'binary',
    outboundMode: 'direct',
    enabled: true,
    isBuiltIn: true
  }
]

const builtInHubRules: HubRuleSet[] = [
  {
    name: 'geosite-google',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-google.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-google.srs'
  },
  {
    name: 'geosite-youtube',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-youtube.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-youtube.srs'
  },
  {
    name: 'geosite-twitter',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-twitter.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-twitter.srs'
  },
  {
    name: 'geosite-facebook',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-facebook.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-facebook.srs'
  },
  {
    name: 'geosite-instagram',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-instagram.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-instagram.srs'
  },
  {
    name: 'geosite-tiktok',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-tiktok.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-tiktok.srs'
  },
  {
    name: 'geosite-netflix',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-netflix.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-netflix.srs'
  },
  {
    name: 'geosite-spotify',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-spotify.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-spotify.srs'
  },
  {
    name: 'geosite-openai',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-openai.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-openai.srs'
  },
  {
    name: 'geosite-github',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-github.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-github.srs'
  },
  {
    name: 'geosite-microsoft',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-microsoft.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-microsoft.srs'
  },
  {
    name: 'geosite-apple',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-apple.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-apple.srs'
  },
  {
    name: 'geosite-amazon',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-amazon.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-amazon.srs'
  },
  {
    name: 'geosite-telegram',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-telegram.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-telegram.srs'
  },
  {
    name: 'geosite-discord',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-discord.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-discord.srs'
  },
  {
    name: 'geosite-whatsapp',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-whatsapp.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-whatsapp.srs'
  },
  {
    name: 'geosite-bilibili',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-bilibili.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-bilibili.srs'
  },
  {
    name: 'geosite-steam',
    tags: ['Official', 'geosite'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-steam.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-steam.srs'
  },
  {
    name: 'geoip-google',
    tags: ['Official', 'geoip'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-google.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-google.srs'
  },
  {
    name: 'geoip-netflix',
    tags: ['Official', 'geoip'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-netflix.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-netflix.srs'
  },
  {
    name: 'geoip-telegram',
    tags: ['Official', 'geoip'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-telegram.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-telegram.srs'
  },
  {
    name: 'geoip-twitter',
    tags: ['Official', 'geoip'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-twitter.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-twitter.srs'
  },
  {
    name: 'geoip-facebook',
    tags: ['Official', 'geoip'],
    sourceUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-facebook.json',
    binaryUrl:
      'https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-facebook.srs'
  }
].map(sanitizeBuiltInHubRule)

export default function RuleSets() {
  const [ruleSets, setRuleSets] = useState<RuleSetItem[]>([])
  const [isRuleSetsLoaded, setIsRuleSetsLoaded] = useState(false)
  const [showAddDialog, setShowAddDialog] = useState(false)
  const [showHubDialog, setShowHubDialog] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)
  const [deleteTargetId, setDeleteTargetId] = useState<string | null>(null)
  const [editingRuleSet, setEditingRuleSet] = useState<RuleSetItem | null>(null)
  const [hubSearch, setHubSearch] = useState('')
  const [hubRuleSets, setHubRuleSets] = useState<HubRuleSet[]>(builtInHubRules)
  const [hubLoading, setHubLoading] = useState(false)
  const [hubError, setHubError] = useState<string | null>(null)

  const [showHubAddConfirm, setShowHubAddConfirm] = useState(false)
  const [hubAddTarget, setHubAddTarget] = useState<{
    hub: HubRuleSet
    format: 'binary' | 'source'
  } | null>(null)

  const toast = useToast()
  const { error: showError } = toast

  // Track which rulesets are currently downloading
  const [downloadingTags, setDownloadingTags] = useState<Set<string>>(new Set())

  const { profiles, loadProfiles } = useProfiles()
  const [isLoadingData, setIsLoadingData] = useState(false)
  const { allNodes, loadAllNodes } = useNodesStore()

  const [dialogData, setDialogData] = useState({
    tag: '',
    name: '',
    url: '',
    type: 'remote' as 'remote' | 'local',
    format: 'binary' as 'binary' | 'source',
    outboundMode: 'proxy' as RuleSetItem['outboundMode'],
    outboundValue: ''
  })


  const [isSaving, setIsSaving] = useState(false)
  const [isApplying, setIsApplying] = useState(false)
  const [updateResult, setUpdateResult] = useState<string | null>(null)
  const operationRef = useRef(false)
  const savedRuleSetsRef = useRef<RuleSetItem[]>([])
  const needsRestart = useConnectionStore(s => s.needsRestart)
  const vpnState = useConnectionStore(s => s.state)
  const busy = isSaving || isApplying || downloadingTags.size > 0

  useEffect(() => {
    window.api.ruleset.list().then((data) => {
      const loaded = data ?? defaultRuleSets
      savedRuleSetsRef.current = loaded
      setRuleSets(loaded)
      setIsRuleSetsLoaded(true)
    }).catch(error => showError(`读取规则集失败: ${String(error)}`))
  }, [showError])

  // Complete persistence before offering restart; never save merely by visiting this page.
  const persistRuleSets = async (next: RuleSetItem[], message: string) => {
    setIsSaving(true)
    try {
      await window.api.ruleset.save(next)
      savedRuleSetsRef.current = next
      setRuleSets(next)
      toast.showRestartToast(message)
      return true
    } catch (error) {
      setRuleSets(savedRuleSetsRef.current)
      toast.error(`保存规则集失败: ${String(error)}`)
      return false
    } finally {
      setIsSaving(false)
    }
  }

  const applyRules = async () => {
    if (operationRef.current || busy) return
    operationRef.current = true
    setIsApplying(true)
    try {
      const result = await window.api.singbox.restart()
      if (!result.success) throw new Error(result.error || '未知错误')
      useConnectionStore.getState().setNeedsRestart(false)
      if (result.warning) toast.warning(result.warning)
      else toast.success('分流配置已应用')
    } catch (error) {
      toast.error(`应用失败: ${String(error)}`)
    } finally {
      operationRef.current = false
      setIsApplying(false)
    }
  }

  const loadProfilesSafe = useCallback(async () => {
    try {
      await loadProfiles()
    } catch (error) {
      console.error('Failed to load profiles:', error)
    }
  }, [loadProfiles])

  const loadAllData = useCallback(async () => {
    setIsLoadingData(true)
    try {
      await Promise.all([loadAllNodes(), loadProfilesSafe()])
    } finally {
      setIsLoadingData(false)
    }
  }, [loadAllNodes, loadProfilesSafe])

  useEffect(() => {
    loadAllData()
  }, [loadAllData])

  const fetchHubRuleSets = useCallback(async () => {
    setHubLoading(true)
    setHubError(null)
    try {
      // Detect Tauri environment
      const isTauri = '__TAURI_INTERNALS__' in window
      
      let data: { tree: HubTreeEntry[] }
      
      if (isTauri && window.api.ruleset.fetchHub) {
        // Use Tauri backend API (proxy support)
        data = await window.api.ruleset.fetchHub()
      } else {
        // Electron: use direct fetch
        const response = await fetch(
          'https://api.github.com/repos/SagerNet/sing-geosite/git/trees/rule-set?recursive=1',
          {
            headers: {
              'User-Agent': 'KunBox-Windows-App'
            }
          }
        )
        if (!response.ok) throw new Error('Failed to fetch')
        data = await response.json()
      }
      
      const newRuleSets = buildHubRuleSets(data.tree)
      setHubRuleSets(newRuleSets.length > 0 ? newRuleSets : builtInHubRules)
    } catch (e) {
      console.error('Failed to fetch hub:', e)
      setHubError('获取规则集列表失败，使用内置列表')
      setHubRuleSets(builtInHubRules)
    } finally {
      setHubLoading(false)
    }
  }, [])

  const filteredHubRules = useMemo(() => {
    const addedTags = new Set(ruleSets.map((r) => r.tag))
    return hubRuleSets.filter(
      (r) =>
        !addedTags.has(r.name) &&
        r.name.toLowerCase().includes(hubSearch.toLowerCase())
    )
  }, [hubSearch, ruleSets, hubRuleSets])

  const toggleRuleSet = async (id: string) => {
    const ruleSet = ruleSets.find(rs => rs.id === id)
    if (!ruleSet || operationRef.current) return
    operationRef.current = true
    setIsSaving(true)
    try {
      if (!ruleSet.enabled && ruleSet.type === 'remote') {
        setDownloadingTags(new Set([ruleSet.tag]))
        const result = await window.api.ruleset.download(ruleSet)
        if (!result.success) throw new Error(result.error || '下载失败')
      }
      await persistRuleSets(
        ruleSets.map(rs => rs.id === id ? { ...rs, enabled: !rs.enabled } : rs),
        `规则集「${ruleSet.name}」已${ruleSet.enabled ? '禁用' : '启用'}`
      )
    } catch (error) {
      toast.error(`启用规则集失败: ${String(error)}`)
    } finally {
      setDownloadingTags(new Set())
      setIsSaving(false)
      operationRef.current = false
    }
  }

  const updateRuleSets = async (targets: RuleSetItem[]) => {
    if (operationRef.current || targets.length === 0) return
    operationRef.current = true
    setDownloadingTags(new Set(targets.map(rs => rs.tag)))
    setUpdateResult(null)
    let updated = 0
    const failures: string[] = []
    try {
      for (const ruleSet of targets) {
        try {
          const result = await window.api.ruleset.download(ruleSet, true)
          if (!result.success) throw new Error(result.error || '下载失败')
          updated++
        } catch (error) {
          failures.push(`${ruleSet.name}: ${String(error)}`)
        }
      }
      const summary = `已更新 ${updated} 个规则集${failures.length ? `，失败 ${failures.length} 个，已有缓存保留` : ''}`
      setUpdateResult([summary, ...failures].join('\n'))
      if (updated > 0) toast.showRestartToast(summary)
      else toast.error(summary)
    } finally {
      setDownloadingTags(new Set())
      operationRef.current = false
    }
  }

  const confirmDelete = (id: string) => {
    setDeleteTargetId(id)
    setShowDeleteConfirm(true)
  }

  const deleteRuleSet = async () => {
    if (!deleteTargetId || operationRef.current) return
    operationRef.current = true
    try {
      const target = ruleSets.find(rs => rs.id === deleteTargetId)
      if (await persistRuleSets(ruleSets.filter(rs => rs.id !== deleteTargetId), `已删除规则集「${target?.name || target?.tag}」`)) {
        setDeleteTargetId(null)
        setShowDeleteConfirm(false)
      }
    } finally {
      operationRef.current = false
    }
  }

  const openEditDialog = (rs: RuleSetItem) => {
    setEditingRuleSet(rs)
    setDialogData({
      tag: rs.tag,
      name: rs.name,
      url: rs.url,
      type: rs.type,
      format: rs.format,
      outboundMode: rs.outboundMode,
      outboundValue: rs.outboundValue || ''
    })
    setShowAddDialog(true)
  }

  const openAddDialog = () => {
    setEditingRuleSet(null)
    setDialogData({
      tag: '',
      name: '',
      url: '',
      type: 'remote',
      format: 'binary',
      outboundMode: 'proxy',
      outboundValue: ''
    })
    setShowAddDialog(true)
  }

  const saveRuleSet = async () => {
    const tag = dialogData.tag.trim()
    if (operationRef.current) return
    if (!/^[a-zA-Z0-9._@-]{1,128}$/.test(tag)) {
      toast.error('标签只能包含字母、数字、点、下划线、@ 或连字符，最多 128 个字符')
      return
    }
    if (ruleSets.some(rs => rs.tag === tag && rs.id !== editingRuleSet?.id)) {
      toast.error('规则集标签已存在')
      return
    }
    if (dialogData.type === 'remote' && !dialogData.url.trim()) {
      toast.error('请输入规则集下载地址')
      return
    }
    if ((dialogData.outboundMode === 'node' || dialogData.outboundMode === 'profile') && !dialogData.outboundValue) {
      toast.error(dialogData.outboundMode === 'node' ? '请选择节点' : '请选择配置')
      return
    }
    const nextRule: RuleSetItem = {
      ...dialogData,
      tag,
      url: dialogData.url.trim(),
      name: dialogData.name.trim() || tag,
      id: editingRuleSet?.id ?? Date.now().toString(),
      outboundValue: dialogData.outboundValue || undefined,
      enabled: editingRuleSet?.enabled ?? true,
      isBuiltIn: editingRuleSet?.isBuiltIn ?? false
    }
    operationRef.current = true
    setIsSaving(true)
    try {
      const sourceChanged = !!editingRuleSet && (
        editingRuleSet.url !== nextRule.url || editingRuleSet.format !== nextRule.format || editingRuleSet.tag !== tag
      )
      if (nextRule.type === 'remote' && (nextRule.enabled || sourceChanged)) {
        setDownloadingTags(new Set([tag]))
        const result = await window.api.ruleset.download(nextRule, sourceChanged || !editingRuleSet)
        if (!result.success) throw new Error(result.error || '下载失败')
      }
      const next = editingRuleSet
        ? ruleSets.map(rs => rs.id === nextRule.id ? nextRule : rs)
        : [...ruleSets, nextRule]
      if (await persistRuleSets(next, `规则集「${nextRule.name}」已保存`)) setShowAddDialog(false)
    } catch (error) {
      toast.error(`保存规则集失败: ${String(error)}`)
    } finally {
      setDownloadingTags(new Set())
      setIsSaving(false)
      operationRef.current = false
    }
  }

  const openHubAddConfirm = (hub: HubRuleSet, format: 'binary' | 'source') => {
    setHubAddTarget({ hub, format })
    setShowHubAddConfirm(true)
  }

  const confirmAddFromHub = async () => {
    if (!hubAddTarget || operationRef.current) return
    const { hub, format } = hubAddTarget
    const url = format === 'binary' ? hub.binaryUrl : hub.sourceUrl
    if (!url || !isHubFormatAvailable(hub, format)) {
      toast.error(`规则集「${hub.name}」没有可用的${format === 'binary' ? 'Binary' : 'Source'}文件`)
      return
    }
    const newRuleSet: RuleSetItem = {
      id: Date.now().toString(), tag: hub.name, name: hub.name, url,
      type: 'remote', format, outboundMode: 'proxy', enabled: true, isBuiltIn: false
    }
    operationRef.current = true
    setDownloadingTags(new Set([hub.name]))
    try {
      const result = await window.api.ruleset.download(newRuleSet, true)
      if (!result.success) throw new Error(result.error || '下载失败')
      if (await persistRuleSets([...ruleSets, newRuleSet], `规则集「${hub.name}」已添加`)) {
        setShowHubAddConfirm(false)
        setHubAddTarget(null)
      }
    } catch (error) {
      toast.error(`规则集「${hub.name}」下载失败: ${String(error)}`)
    } finally {
      setDownloadingTags(new Set())
      operationRef.current = false
    }
  }

  const resetToDefaults = async () => {
    if (operationRef.current) return
    operationRef.current = true
    setIsSaving(true)
    try {
      for (const ruleSet of defaultRuleSets.filter(rs => rs.enabled)) {
        const result = await window.api.ruleset.download(ruleSet)
        if (!result.success) throw new Error(result.error || '下载失败')
      }
      await persistRuleSets(defaultRuleSets, '已重置为默认规则集')
    } catch (error) {
      toast.error(`重置规则集失败: ${String(error)}`)
    } finally {
      setIsSaving(false)
      operationRef.current = false
    }
  }

  const getModeIcon = (mode: RuleSetItem['outboundMode']) => {
    switch (mode) {
      case 'direct':
        return <Globe className="w-3.5 h-3.5" />
      case 'proxy':
        return <Zap className="w-3.5 h-3.5" />
      case 'block':
        return <Ban className="w-3.5 h-3.5" />
      case 'node':
        return <Server className="w-3.5 h-3.5" />
      case 'profile':
        return <FileText className="w-3.5 h-3.5" />
    }
  }

  const getModeStyle = (mode: RuleSetItem['outboundMode']) => {
    switch (mode) {
      case 'direct':
        return 'text-emerald-400 bg-emerald-500/15 border-emerald-500/30'
      case 'proxy':
        return 'text-violet-400 bg-violet-500/15 border-violet-500/30'
      case 'block':
        return 'text-red-400 bg-red-500/15 border-red-500/30'
      case 'node':
        return 'text-orange-400 bg-orange-500/15 border-orange-500/30'
      case 'profile':
        return 'text-cyan-400 bg-cyan-500/15 border-cyan-500/30'
    }
  }

  const getModeLabel = (mode: RuleSetItem['outboundMode']) => {
    switch (mode) {
      case 'direct':
        return '直连'
      case 'proxy':
        return '代理'
      case 'block':
        return '拦截'
      case 'node':
        return '节点'
      case 'profile':
        return '配置'
    }
  }

  // Get display name for outboundValue (resolve profile ID to name)
  const getOutboundValueDisplay = (ruleSet: RuleSetItem): string | null => {
    if (!ruleSet.outboundValue) return null
    if (ruleSet.outboundMode === 'profile') {
      // Find profile name by ID
      const profile = profiles.find(p => p.id === ruleSet.outboundValue)
      return profile?.name || ruleSet.outboundValue
    }
    if (ruleSet.outboundMode === 'node') {
      const parts = ruleSet.outboundValue.split('::')
      if (parts.length === 2) {
        const [profileId, nodeTag] = parts
        const profile = profiles.find(p => p.id === profileId)
        if (profile) {
          return `${nodeTag} (${profile.name})`
        }
        return nodeTag
      }
    }
    return ruleSet.outboundValue
  }

  return (
    <fieldset disabled={busy || !isRuleSetsLoaded} className="min-w-0 h-full flex flex-col px-6 pb-6 overflow-y-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-4">
          <div className="w-14 h-14 rounded-2xl bg-gradient-to-br from-[var(--accent-primary)]/20 to-[var(--accent-primary)]/5 flex items-center justify-center border border-[var(--accent-primary)]/20">
            <Shield className="w-7 h-7 text-[var(--accent-primary)]" />
          </div>
          <div className="space-y-1">
            <h2 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">
              规则集
            </h2>
            <p className="text-[var(--text-muted)] text-sm font-medium">
              管理流量路由规则，优先级从上至下
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => updateRuleSets(ruleSets.filter(rs => rs.type === 'remote'))}
            disabled={!ruleSets.some(rs => rs.type === 'remote')}
            className="h-10 px-4 rounded-xl text-sm font-medium flex items-center gap-2 bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] disabled:opacity-50"
          >
            <RefreshCw className={`w-4 h-4 ${downloadingTags.size > 0 ? 'animate-spin' : ''}`} />
            {downloadingTags.size > 0 ? '下载中…' : '更新全部'}
          </button>
          <button
            onClick={() => {
              setShowHubDialog(true)
              fetchHubRuleSets()
            }}
            className="h-10 px-4 rounded-xl text-sm font-medium flex items-center gap-2 bg-[var(--bg-secondary)] text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] border border-[var(--glass-border)] transition-colors duration-150 active:scale-[0.98]"
          >
            <Download className="w-4 h-4" />
            仓库
          </button>
          <button
            onClick={resetToDefaults}
            className="h-10 px-4 rounded-xl text-sm font-medium flex items-center gap-2 bg-[var(--bg-secondary)] text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] border border-[var(--glass-border)] transition-colors duration-150 active:scale-[0.98]"
          >
            <RefreshCw className="w-4 h-4" />
            重置
          </button>
          <button
            onClick={openAddDialog}
            className="h-10 px-4 rounded-xl text-sm font-medium flex items-center gap-2 bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90 shadow-lg shadow-[var(--accent-primary)]/20 transition-colors duration-150 active:scale-[0.98]"
          >
            <Plus className="w-4 h-4" />
            添加规则
          </button>
        </div>
      </div>

      <RoutingModeNotice />
      {needsRestart && vpnState === 'connected' && (
        <div className="mb-4 p-3 rounded-xl bg-amber-500/15 text-amber-500 flex items-center justify-between gap-3">
          <span className="text-sm">配置已保存，重启 VPN 后完整应用分流规则（会短暂断线）</span>
          <button onClick={applyRules} className="shrink-0 px-3 py-1.5 rounded-lg bg-amber-500/20 text-sm">
            {isApplying ? '应用中…' : '立即应用'}
          </button>
        </div>
      )}
      {updateResult && (
        <p role="status" className="mb-4 text-sm text-[var(--text-muted)] whitespace-pre-wrap break-words">{updateResult}</p>
      )}

      {/* Rule Sets List with Drag & Drop */}
      <div className="glass-card p-4 rounded-2xl border border-[var(--glass-border)]">
        <div className="flex items-center gap-2 mb-4">
          <div className="w-8 h-8 rounded-lg bg-[var(--accent-muted)] flex items-center justify-center">
            <Shield className="w-4 h-4 text-[var(--accent-primary)]" />
          </div>
          <span className="text-sm font-semibold text-[var(--text-primary)]">
            路由规则集
          </span>
          <span className="text-xs text-[var(--text-muted)] bg-[var(--bg-tertiary)] px-2 py-0.5 rounded-full">
            {ruleSets.length} 条规则
          </span>
        </div>

        <Reorder.Group
          axis="y"
          values={ruleSets}
          onReorder={setRuleSets}
          className="space-y-2"
        >
          {ruleSets.map((ruleSet) => (
            <Reorder.Item
              key={ruleSet.id}
              value={ruleSet}
              dragListener={!busy}
              onDragEnd={() => {
                if (!operationRef.current && ruleSets !== savedRuleSetsRef.current) {
                  operationRef.current = true
                  void persistRuleSets(ruleSets, '规则集优先级已保存').finally(() => { operationRef.current = false })
                }
              }}
              transition={fastTransition}
              whileDrag={{
                scale: 1.01,
                boxShadow: '0 8px 20px -8px rgba(0,0,0,0.25)',
                zIndex: 10
              }}
              className={`flex items-center gap-3 p-3 rounded-xl bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] border border-[var(--glass-border)] transition-colors duration-150 cursor-grab active:cursor-grabbing group ${
                !ruleSet.enabled && 'opacity-50'
              }`}
            >
              <div className="flex-shrink-0 text-[var(--text-faint)] group-hover:text-[var(--text-muted)] transition-colors duration-150">
                <GripVertical className="w-4 h-4" />
              </div>

                {downloadingTags.has(ruleSet.tag) ? (
                  <div className="w-10 h-6 flex items-center justify-center flex-shrink-0">
                    <Loader2 className="w-5 h-5 text-[var(--accent-primary)] animate-spin" />
                  </div>
                ) : (
                  <Switch.Root
                    checked={ruleSet.enabled}
                    onCheckedChange={() => toggleRuleSet(ruleSet.id)}
                    className="w-10 h-6 rounded-full bg-[var(--bg-tertiary)] data-[state=checked]:bg-[var(--accent-primary)] transition-colors flex-shrink-0"
                  >
                    <Switch.Thumb className="block w-4 h-4 bg-white rounded-full transition-transform translate-x-1 data-[state=checked]:translate-x-5 shadow-sm" />
                  </Switch.Root>
                )}

                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 mb-0.5 flex-wrap">
                    <span className="text-sm font-semibold text-[var(--text-primary)] truncate">
                      {ruleSet.name}
                    </span>
                    <span className="px-1.5 py-0.5 text-[10px] bg-black/20 rounded text-[var(--text-muted)] font-mono">
                      {ruleSet.tag}
                    </span>
                    <span className="px-1.5 py-0.5 text-[10px] bg-blue-500/20 text-blue-400 rounded font-semibold">
                      {ruleSet.format === 'binary' ? 'SRS' : 'JSON'}
                    </span>
                    <span className="px-1.5 py-0.5 text-[10px] bg-black/20 rounded text-[var(--text-muted)]">
                      {ruleSet.type === 'remote' ? '远程' : '本地'}
                    </span>
                    {ruleSet.isBuiltIn && (
                      <span className="px-1.5 py-0.5 text-[10px] bg-emerald-500/20 text-emerald-400 rounded">
                        内置
                      </span>
                    )}
                  </div>
                  <p className="text-[10px] text-[var(--text-faint)] truncate max-w-[400px]">
                    {ruleSet.url}
                  </p>
                </div>

                <div
                  className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border text-xs font-semibold ${getModeStyle(ruleSet.outboundMode)}`}
                >
                  {getModeIcon(ruleSet.outboundMode)}
                  <span>
                    {getModeLabel(ruleSet.outboundMode)}
                    {getOutboundValueDisplay(ruleSet) && (
                      <span className="ml-1 opacity-80">: {getOutboundValueDisplay(ruleSet)}</span>
                    )}
                  </span>
                </div>

                <div className="flex items-center gap-1">
                  {ruleSet.type === 'remote' && (
                    <button
                      onClick={() => updateRuleSets([ruleSet])}
                      className="p-2 rounded-lg hover:bg-[var(--bg-hover)] transition-colors duration-150"
                      title="更新规则集（重新下载）"
                      aria-label={`更新规则集 ${ruleSet.name}`}
                    >
                      <RefreshCw className={`w-4 h-4 text-[var(--text-muted)] ${downloadingTags.has(ruleSet.tag) ? 'animate-spin' : ''}`} />
                    </button>
                  )}
                  <button
                    onClick={() => openEditDialog(ruleSet)}
                    className="p-2 rounded-lg hover:bg-[var(--bg-hover)] transition-colors duration-150"
                    title="编辑"
                  >
                    <Edit2 className="w-4 h-4 text-[var(--text-muted)]" />
                  </button>
                  <button
                    onClick={() => confirmDelete(ruleSet.id)}
                    className="p-2 rounded-lg hover:bg-red-500/10 transition-colors duration-150"
                    title="删除"
                  >
                    <Trash2 className="w-4 h-4 text-red-400" />
                  </button>
                </div>
              </Reorder.Item>
            ))}
        </Reorder.Group>

        {ruleSets.length === 0 && (
          <div className="text-center py-12 text-[var(--text-muted)]">
            <Shield className="w-12 h-12 mx-auto mb-3 opacity-30" />
            <p>暂无规则集，点击上方按钮添加</p>
          </div>
        )}

        <p className="text-xs text-[var(--text-faint)] mt-4">
          拖拽可调整优先级。更新会重新下载远程规则集，失败时保留已有缓存；修改分流配置后请点击「立即应用」。
        </p>
      </div>

      {/* Legend */}
      <div className="mt-6 glass-card p-5 rounded-2xl border border-[var(--glass-border)]">
        <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-4">
          出站模式说明
        </h3>
        <div className="flex flex-wrap gap-4 text-sm">
          {(['direct', 'proxy', 'block', 'node', 'profile'] as const).map(
            (mode) => (
              <div
                key={mode}
                className={`inline-flex items-center gap-2 px-3 py-2 rounded-xl border ${getModeStyle(mode)}`}
              >
                {getModeIcon(mode)}
                <span className="font-semibold">{getModeLabel(mode)}</span>
              </div>
            )
          )}
        </div>
      </div>

      {/* Add/Edit Modal */}
      <Modal
        isOpen={showAddDialog}
        onClose={() => { if (!busy) setShowAddDialog(false) }}
        title={editingRuleSet ? '编辑规则集' : '添加规则集'}
        maxWidth="max-w-md"
        footer={
          <>
            <ModalButton variant="ghost" disabled={busy} onClick={() => setShowAddDialog(false)}>
              取消
            </ModalButton>
            <ModalButton onClick={saveRuleSet} disabled={busy || !dialogData.tag.trim()}>
              {busy ? '保存中…' : '保存'}
            </ModalButton>
          </>
        }
      >
        <div className="space-y-4">
          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              标签 (Tag) *
            </label>
            <input
              type="text"
              value={dialogData.tag}
              onChange={(e) =>
                setDialogData({ ...dialogData, tag: e.target.value })
              }
              placeholder="例如: geosite-cn"
              className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent-primary)] transition-colors"
            />
          </div>

          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              名称 (可选)
            </label>
            <input
              type="text"
              value={dialogData.name}
              onChange={(e) =>
                setDialogData({ ...dialogData, name: e.target.value })
              }
              placeholder="例如: 中国网站"
              className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent-primary)] transition-colors"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1.5">
                类型
              </label>
              <AppSelect
                value={dialogData.type}
                options={RULESET_TYPE_OPTIONS}
                onValueChange={(value) =>
                  setDialogData({
                    ...dialogData,
                    type: value as 'remote' | 'local'
                  })
                }
                ariaLabel="选择规则集类型"
              />
            </div>
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1.5">
                格式
              </label>
              <AppSelect
                value={dialogData.format}
                options={RULESET_FORMAT_OPTIONS}
                onValueChange={(value) =>
                  setDialogData({
                    ...dialogData,
                    format: value as 'binary' | 'source'
                  })
                }
                ariaLabel="选择规则集格式"
              />
            </div>
          </div>

          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              {dialogData.type === 'remote' ? 'URL' : '本地路径'}
            </label>
            <input
              type="text"
              value={dialogData.url}
              onChange={(e) =>
                setDialogData({ ...dialogData, url: e.target.value })
              }
              placeholder={
                dialogData.type === 'remote'
                  ? 'https://...'
                  : 'C:\\path\\to\\rules.srs'
              }
              className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent-primary)] transition-colors"
            />
          </div>

          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              出站模式
            </label>
            <AppSelect
              value={dialogData.outboundMode}
              options={RULESET_OUTBOUND_OPTIONS}
              onValueChange={(value) =>
                setDialogData({
                  ...dialogData,
                  outboundMode: value as RuleSetItem['outboundMode'],
                  outboundValue: ''
                })
              }
              ariaLabel="选择出站模式"
            />
          </div>

          {/* Node/Profile Selector */}
          {(dialogData.outboundMode === 'node' || dialogData.outboundMode === 'profile') && (
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1.5">
                {dialogData.outboundMode === 'node' ? '选择节点' : '选择配置'}
              </label>
              {isLoadingData ? (
                <div className="flex items-center gap-2 h-10 px-3 text-[var(--text-muted)]">
                  <Loader2 className="w-4 h-4 animate-spin" />
                  加载中...
                </div>
              ) : (
                <AppSelect
                  value={dialogData.outboundValue}
                  onValueChange={(value) =>
                    setDialogData({ ...dialogData, outboundValue: value })
                  }
                  options={dialogData.outboundMode === 'node'
                    ? allNodes
                        .filter((n) => n.tag)
                        .map((node) => ({
                          value: `${node.sourceProfileId}::${node.tag}`,
                          label: `${node.tag}（${node.type}）· ${node.sourceProfileName}`,
                        }))
                    : profiles.map((profile) => ({
                        value: profile.id,
                        label: profile.name,
                      }))}
                  placeholder={`请选择${dialogData.outboundMode === 'node' ? '节点' : '配置'}`}
                  ariaLabel={`选择${dialogData.outboundMode === 'node' ? '节点' : '配置'}`}
                />
              )}
              {dialogData.outboundMode === 'node' && allNodes.filter((n) => n.tag).length === 0 && !isLoadingData && (
                <p className="text-xs text-amber-400 mt-1">暂无可用节点</p>
              )}
              {dialogData.outboundMode === 'profile' && profiles.length === 0 && !isLoadingData && (
                <p className="text-xs text-amber-400 mt-1">暂无可用配置</p>
              )}
            </div>
          )}
        </div>
      </Modal>

      {/* Hub Modal */}
      <Modal
        isOpen={showHubDialog}
        onClose={() => setShowHubDialog(false)}
        title={
          <div className="flex items-center gap-2">
            <Download className="w-5 h-5 text-[var(--accent-primary)]" />
            规则集仓库
          </div>
        }
        maxWidth="max-w-2xl"
      >
        <div className="space-y-4">
          <div className="flex items-center gap-2 bg-[var(--bg-secondary)] rounded-xl px-3 border border-[var(--glass-border)]">
            <Search className="w-4 h-4 text-[var(--text-muted)]" />
            <input
              type="text"
              value={hubSearch}
              onChange={(e) => setHubSearch(e.target.value)}
              placeholder="搜索规则集..."
              className="flex-1 h-10 bg-transparent text-[var(--text-primary)] border-0 outline-none placeholder:text-[var(--text-faint)]"
            />
            {hubSearch && (
              <button
                onClick={() => setHubSearch('')}
                className="p-1 hover:bg-[var(--bg-tertiary)] rounded"
              >
                <X className="w-4 h-4 text-[var(--text-muted)]" />
              </button>
            )}
          </div>

          <div className="flex items-center justify-between">
            <span className="text-sm text-[var(--text-muted)]">
              {filteredHubRules.length} 个可用规则集
            </span>
            <button
              onClick={fetchHubRuleSets}
              disabled={hubLoading}
              className="text-sm text-[var(--accent-primary)] hover:underline flex items-center gap-1"
            >
              {hubLoading ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <RefreshCw className="w-4 h-4" />
              )}
              刷新列表
            </button>
          </div>

          {hubError && (
            <div className="flex items-center gap-2 text-amber-400 text-sm bg-amber-500/10 px-3 py-2 rounded-lg">
              <AlertCircle className="w-4 h-4" />
              {hubError}
            </div>
          )}

          <div className="max-h-[400px] overflow-y-auto space-y-2 pr-1">
            {hubLoading ? (
              <div className="flex items-center justify-center py-12">
                <Loader2 className="w-8 h-8 animate-spin text-[var(--accent-primary)]" />
              </div>
            ) : (
              filteredHubRules.map((hub) => (
                <div
                  key={hub.name}
                  className="flex items-center justify-between p-3 rounded-xl bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] border border-[var(--glass-border)] transition-colors duration-150"
                >
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-[var(--text-primary)]">
                      {hub.name}
                    </span>
                    {hub.tags.map((tag) => (
                      <span
                        key={tag}
                        className="px-1.5 py-0.5 text-[10px] bg-[var(--accent-muted)] text-[var(--accent-primary)] rounded font-medium"
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                  <div className="flex items-center gap-2">
                    {isHubFormatAvailable(hub, 'source') && <button
                      onClick={() => openHubAddConfirm(hub, 'source')}
                      className="text-xs px-2.5 py-1.5 rounded-lg text-[var(--accent-primary)] hover:bg-[var(--accent-primary)]/10 border border-[var(--accent-primary)]/30 transition-colors duration-150 flex items-center gap-1 active:scale-[0.97]"
                    >
                      <Plus className="w-3 h-3" />
                      Source
                    </button>}
                    {isHubFormatAvailable(hub, 'binary') && <button
                      onClick={() => openHubAddConfirm(hub, 'binary')}
                      className="text-xs px-2.5 py-1.5 rounded-lg bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90 transition-colors duration-150 flex items-center gap-1 active:scale-[0.97]"
                    >
                      <Plus className="w-3 h-3" />
                      Binary
                    </button>}
                  </div>
                </div>
              ))
            )}
            {!hubLoading && filteredHubRules.length === 0 && (
              <div className="text-center py-8 text-[var(--text-muted)]">
                <CheckCircle className="w-10 h-10 mx-auto mb-2 opacity-30" />
                <p>没有找到可用的规则集</p>
                <p className="text-xs mt-1">所有规则集都已添加或无匹配结果</p>
              </div>
            )}
          </div>

          <div className="pt-3 border-t border-[var(--glass-border)]">
            <a
              href="https://github.com/SagerNet/sing-geosite"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs text-[var(--text-muted)] hover:text-[var(--accent-primary)] flex items-center gap-1"
            >
              <ExternalLink className="w-3 h-3" />
              规则集来源: SagerNet/sing-geosite
            </a>
          </div>
        </div>
      </Modal>

      {/* Delete Confirm Modal */}
      <Modal
        isOpen={showDeleteConfirm}
        onClose={() => { if (!busy) setShowDeleteConfirm(false) }}
        title="确认删除"
        maxWidth="max-w-sm"
        footer={
          <>
            <ModalButton
              variant="ghost"
              onClick={() => setShowDeleteConfirm(false)}
            >
              取消
            </ModalButton>
            <ModalButton variant="danger" disabled={busy} onClick={deleteRuleSet}>
              删除
            </ModalButton>
          </>
        }
      >
        <p className="text-[var(--text-secondary)]">
          确定要删除这个规则集吗？此操作无法撤销。
        </p>
      </Modal>

      {/* Hub Add Confirm Modal */}
      <Modal
        isOpen={showHubAddConfirm}
        onClose={() => {
          if (busy) return
          setShowHubAddConfirm(false)
          setHubAddTarget(null)
        }}
        title="确认添加规则集"
        maxWidth="max-w-sm"
        footer={
          <>
            <ModalButton
              variant="ghost"
              disabled={busy}
              onClick={() => {
                setShowHubAddConfirm(false)
                setHubAddTarget(null)
              }}
            >
              取消
            </ModalButton>
            <ModalButton disabled={busy} onClick={confirmAddFromHub}>
              <Check className="w-4 h-4" />
              确认添加
            </ModalButton>
          </>
        }
      >
        {hubAddTarget && (
          <div className="space-y-3">
            <p className="text-[var(--text-secondary)]">
              确定要添加以下规则集吗？
            </p>
            <div className="p-3 rounded-xl bg-[var(--bg-secondary)] border border-[var(--glass-border)]">
              <div className="flex items-center gap-2 mb-2">
                <span className="text-sm font-semibold text-[var(--text-primary)]">
                  {hubAddTarget.hub.name}
                </span>
                <span className="px-1.5 py-0.5 text-[10px] bg-blue-500/20 text-blue-400 rounded font-semibold">
                  {hubAddTarget.format === 'binary' ? 'SRS' : 'JSON'}
                </span>
              </div>
              <p className="text-[10px] text-[var(--text-faint)] break-all">
                {hubAddTarget.format === 'binary'
                  ? hubAddTarget.hub.binaryUrl
                  : hubAddTarget.hub.sourceUrl}
              </p>
            </div>
          </div>
        )}
      </Modal>

    </fieldset>
  )
}
