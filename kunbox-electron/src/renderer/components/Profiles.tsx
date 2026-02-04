import { useState, useEffect, useRef } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Plus, RefreshCw, Trash2, Check, Loader2, FolderOpen, MoreVertical, Edit3, ToggleLeft, ToggleRight } from 'lucide-react'
import type { Profile } from '@shared/types'
import { ConfirmModal } from './ui/ConfirmModal'
import { EditProfileModal } from './ui/EditProfileModal'
import { AddProfileModal } from './ui/AddProfileModal'

export default function Profiles() {
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [activeId, setActiveId] = useState<string | null>(null)
  const [addModalOpen, setAddModalOpen] = useState(false)
  const [openMenuId, setOpenMenuId] = useState<string | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  const [deleteModalOpen, setDeleteModalOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<Profile | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  const [editModalOpen, setEditModalOpen] = useState(false)
  const [editTarget, setEditTarget] = useState<Profile | null>(null)
  const [isEditing, setIsEditing] = useState(false)

  const [updatingIds, setUpdatingIds] = useState<Set<string>>(new Set())

  useEffect(() => {
    loadProfiles()
  }, [])

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setOpenMenuId(null)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  const loadProfiles = async () => {
    const list = await window.api.profile.list()
    setProfiles(list)
  }

  const handleImportUrl = async (name: string, url: string, settings: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }) => {
    await window.api.profile.add(url, name || undefined, settings)
    await loadProfiles()
  }

  const handleImportContent = async (name: string, content: string, settings: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }) => {
    await window.api.profile.importContent(name, content, settings)
    await loadProfiles()
  }

  const handleRefresh = async (id: string) => {
    setOpenMenuId(null)
    setUpdatingIds((prev) => new Set(prev).add(id))
    try {
      await window.api.profile.update(id)
      await loadProfiles()
    } finally {
      setUpdatingIds((prev) => {
        const next = new Set(prev)
        next.delete(id)
        return next
      })
    }
  }

  const openDeleteModal = (profile: Profile) => {
    setOpenMenuId(null)
    setDeleteTarget(profile)
    setDeleteModalOpen(true)
  }

  const handleConfirmDelete = async () => {
    if (!deleteTarget) return
    setIsDeleting(true)
    try {
      await window.api.profile.delete(deleteTarget.id)
      await loadProfiles()
      setDeleteModalOpen(false)
      setDeleteTarget(null)
    } finally {
      setIsDeleting(false)
    }
  }

  const openEditModal = (profile: Profile) => {
    setOpenMenuId(null)
    setEditTarget(profile)
    setEditModalOpen(true)
  }

  const handleSaveEdit = async (name: string, url: string, settings: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }) => {
    if (!editTarget) return
    setIsEditing(true)
    try {
      await window.api.profile.edit(editTarget.id, { name, url, ...settings })
      await loadProfiles()
      setEditModalOpen(false)
      setEditTarget(null)
    } finally {
      setIsEditing(false)
    }
  }

  const handleToggleEnabled = async (profile: Profile) => {
    setOpenMenuId(null)
    await window.api.profile.setEnabled(profile.id, !profile.enabled)
    await loadProfiles()
  }

  const handleSelect = async (id: string) => {
    await window.api.profile.setActive(id)
    setActiveId(id)
  }

  const formatDate = (timestamp?: number) => {
    if (!timestamp) return '-'
    return new Date(timestamp).toLocaleDateString('zh-CN', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    })
  }

  return (
    <div className="h-full flex flex-col px-6 pb-6">
      <div className="flex items-center justify-between mb-10">
        <div className="space-y-1">
          <h2 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">订阅管理</h2>
          <p className="text-[var(--text-muted)] text-sm font-medium">管理代理订阅与配置文件</p>
        </div>
        <button
          onClick={() => setAddModalOpen(true)}
          className="glass-btn glass-btn-primary h-11 px-6 rounded-xl font-bold text-sm flex items-center gap-2"
        >
          <Plus className="w-4 h-4" />
          添加订阅
        </button>
      </div>

      <div className="flex-1 overflow-auto">
        {profiles.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center">
            <div className="w-20 h-20 rounded-3xl bg-[var(--bg-elevated)] flex items-center justify-center mb-4">
              <FolderOpen className="w-9 h-9 text-[var(--text-faint)]" />
            </div>
            <p className="text-xl font-semibold text-[var(--text-primary)]">暂无订阅</p>
            <p className="text-sm text-[var(--text-faint)] mt-1">点击上方按钮添加订阅</p>
          </div>
        ) : (
          <div className="space-y-4">
            {profiles.map((profile) => {
              const isUpdating = updatingIds.has(profile.id)
              const isDisabled = !profile.enabled

              return (
                <motion.div
                  key={profile.id}
                  onClick={() => !isDisabled && handleSelect(profile.id)}
                  className={`group/card glass-card p-5 rounded-2xl cursor-pointer relative ${
                    openMenuId === profile.id ? 'z-50' : 'z-0'
                  } ${activeId === profile.id ? 'border-[var(--accent-primary)]/50 bg-[var(--accent-muted)]' : ''} ${
                    isDisabled ? 'opacity-50' : ''
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-4">
                      <div className="w-10 h-10 flex items-center justify-center relative">
                        <div
                          className={`w-8 h-8 rounded-full border-2 border-[var(--border-secondary)] transition-opacity duration-150 ${
                            activeId === profile.id ? 'opacity-0' : 'opacity-100'
                          }`}
                        />
                        <AnimatePresence>
                          {activeId === profile.id && (
                            <motion.div
                              initial={{ scale: 0 }}
                              animate={{ scale: 1 }}
                              exit={{ scale: 0 }}
                              transition={{ duration: 0.15 }}
                              className="absolute inset-0 m-auto w-8 h-8 rounded-full bg-[var(--accent-primary)] flex items-center justify-center"
                            >
                              <Check className="w-4 h-4 text-white" />
                            </motion.div>
                          )}
                        </AnimatePresence>
                      </div>

                      <div>
                        <div className="flex items-center gap-2">
                          <p className="text-base font-semibold text-[var(--text-primary)]">{profile.name}</p>
                          {isUpdating && <Loader2 className="w-4 h-4 animate-spin text-[var(--accent-primary)]" />}
                          {isDisabled && (
                            <span className="text-xs px-2 py-0.5 rounded-md bg-[var(--text-muted)]/20 text-[var(--text-muted)]">
                              已禁用
                            </span>
                          )}
                        </div>
                        <p className="text-sm text-[var(--text-muted)]">
                          {profile.nodeCount} 个节点 · 更新于 {formatDate(profile.lastUpdate)}
                        </p>
                      </div>
                    </div>

                    <div
                      className="relative"
                      ref={openMenuId === profile.id ? menuRef : null}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <button
                        onClick={() => setOpenMenuId(openMenuId === profile.id ? null : profile.id)}
                        className="glass-btn w-10 h-10 !p-0 flex items-center justify-center rounded-xl"
                      >
                        <MoreVertical className="w-4 h-4 text-[var(--text-secondary)]" />
                      </button>

                      <div
                        className={`absolute right-0 top-12 z-[100] w-32 py-2 glass-card rounded-xl border border-[var(--glass-border)] shadow-xl origin-top-right transition-all duration-150 ${
                          openMenuId === profile.id
                            ? 'opacity-100 scale-100 pointer-events-auto'
                            : 'opacity-0 scale-90 pointer-events-none'
                        }`}
                      >
                        <button
                          onClick={() => handleRefresh(profile.id)}
                          disabled={isUpdating}
                          className="w-full px-4 py-2.5 text-sm text-[var(--text-primary)] hover:bg-[var(--bg-elevated)] flex items-center gap-3 transition-colors disabled:opacity-50"
                        >
                          <RefreshCw className={`w-4 h-4 ${isUpdating ? 'animate-spin' : ''}`} />
                          更新
                        </button>
                        <button
                          onClick={() => openEditModal(profile)}
                          className="w-full px-4 py-2.5 text-sm text-[var(--text-primary)] hover:bg-[var(--bg-elevated)] flex items-center gap-3 transition-colors"
                        >
                          <Edit3 className="w-4 h-4" />
                          编辑
                        </button>
                        <button
                          onClick={() => handleToggleEnabled(profile)}
                          className="w-full px-4 py-2.5 text-sm text-[var(--text-primary)] hover:bg-[var(--bg-elevated)] flex items-center gap-3 transition-colors"
                        >
                          {profile.enabled ? (
                            <>
                              <ToggleLeft className="w-4 h-4" />
                              禁用
                            </>
                          ) : (
                            <>
                              <ToggleRight className="w-4 h-4" />
                              启用
                            </>
                          )}
                        </button>
                        <div className="my-1 mx-3 border-t border-[var(--border-secondary)]" />
                        <button
                          onClick={() => openDeleteModal(profile)}
                          className="w-full px-4 py-2.5 text-sm text-[var(--status-error)] hover:bg-[var(--status-error)]/10 flex items-center gap-3 transition-colors"
                        >
                          <Trash2 className="w-4 h-4" />
                          删除
                        </button>
                      </div>
                    </div>
                  </div>
                </motion.div>
              )
            })}
          </div>
        )}
      </div>

      <ConfirmModal
        isOpen={deleteModalOpen}
        onClose={() => {
          setDeleteModalOpen(false)
          setDeleteTarget(null)
        }}
        onConfirm={handleConfirmDelete}
        title="删除订阅"
        description={`确定要删除「${deleteTarget?.name || ''}」吗？此操作不可撤销，相关节点数据也将被删除。`}
        confirmText="删除"
        cancelText="取消"
        variant="danger"
        isLoading={isDeleting}
      />

      <EditProfileModal
        isOpen={editModalOpen}
        onClose={() => {
          setEditModalOpen(false)
          setEditTarget(null)
        }}
        onSave={handleSaveEdit}
        initialName={editTarget?.name || ''}
        initialUrl={editTarget?.url || ''}
        initialAutoUpdateInterval={editTarget?.autoUpdateInterval || 0}
        initialDnsPreResolve={editTarget?.dnsPreResolve || false}
        initialDnsServer={editTarget?.dnsServer || null}
        isLoading={isEditing}
      />

      <AddProfileModal
        isOpen={addModalOpen}
        onClose={() => setAddModalOpen(false)}
        onImportUrl={handleImportUrl}
        onImportContent={handleImportContent}
      />
    </div>
  )
}
