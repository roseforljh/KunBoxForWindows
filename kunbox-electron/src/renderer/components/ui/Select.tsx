import { useEffect, useRef, useState } from 'react'
import * as RadixSelect from '@radix-ui/react-select'
import { Check, ChevronDown, Search } from 'lucide-react'

export interface SelectOption {
  value: string
  label: string
  disabled?: boolean
}

interface AppSelectProps {
  value: string
  options: readonly SelectOption[]
  onValueChange: (value: string) => void
  placeholder?: string
  disabled?: boolean
  ariaLabel?: string
  className?: string
}

const EMPTY_VALUE = '__kunbox_select_empty_value__'

function toInternalValue(value: string): string {
  return value === '' ? EMPTY_VALUE : value
}

export function AppSelect({
  value,
  options,
  onValueChange,
  placeholder = '请选择',
  disabled = false,
  ariaLabel,
  className = '',
}: AppSelectProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const searchInputRef = useRef<HTMLInputElement>(null)
  const hasEmptyOption = options.some((option) => option.value === '')
  const selectedValue = value === '' && !hasEmptyOption ? undefined : toInternalValue(value)
  const normalizedSearch = search.trim().toLocaleLowerCase()
  const matchesSearch = (option: SelectOption) =>
    !normalizedSearch ||
    `${option.label} ${option.value}`.toLocaleLowerCase().includes(normalizedSearch)
  const hasMatchingOption = options.some(matchesSearch)

  useEffect(() => {
    if (!open) return

    const focusTimer = window.setTimeout(() => {
      searchInputRef.current?.focus({ preventScroll: true })
    })
    return () => window.clearTimeout(focusTimer)
  }, [open])

  return (
    <RadixSelect.Root
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen)
        if (!nextOpen) setSearch('')
      }}
      value={selectedValue}
      onValueChange={(nextValue) =>
        onValueChange(nextValue === EMPTY_VALUE ? '' : nextValue)
      }
      disabled={disabled}
    >
      <RadixSelect.Trigger
        aria-label={ariaLabel}
        className={`glass-select group inline-flex h-10 min-w-0 w-full items-center justify-between gap-3 rounded-xl px-3 !py-0 text-left text-sm text-[var(--text-primary)] shadow-sm outline-none transition-colors hover:bg-[var(--bg-hover)] focus:border-[var(--accent-primary)] focus:ring-2 focus:ring-[var(--accent-primary)]/15 data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50 data-[placeholder]:text-[var(--text-faint)] ${className}`}
      >
        <RadixSelect.Value className="truncate" placeholder={placeholder} />
        <RadixSelect.Icon asChild>
          <ChevronDown className="h-4 w-4 shrink-0 text-[var(--text-muted)] transition-transform group-data-[state=open]:rotate-180" />
        </RadixSelect.Icon>
      </RadixSelect.Trigger>

      <RadixSelect.Portal>
        <RadixSelect.Content
          position="popper"
          sideOffset={6}
          collisionPadding={12}
          className="floating-surface z-[100] min-w-[var(--radix-select-trigger-width)] max-w-[calc(100vw-24px)] overflow-hidden p-2"
          style={{
            maxHeight: 'min(26rem, var(--radix-select-content-available-height))',
          }}
        >
          <div className="shrink-0 pb-2">
            <div className="relative">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]" />
              <input
                ref={searchInputRef}
                type="text"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                onKeyDown={(event) => {
                  if (!['Escape', 'Tab', 'ArrowUp', 'ArrowDown'].includes(event.key)) {
                    event.stopPropagation()
                  }
                }}
                placeholder="搜索选项..."
                aria-label="搜索选项"
                className="h-10 w-full rounded-xl border-0 bg-[var(--floating-search-bg)] py-0 pl-9 pr-3 text-sm text-[var(--text-primary)] shadow-inner outline-none placeholder:text-[var(--text-faint)] focus:ring-2 focus:ring-[var(--accent-primary)]/20"
              />
            </div>
          </div>

          <RadixSelect.Viewport className="min-h-0 max-h-[22.5rem] flex-1 overflow-y-auto pr-1">
            {/* ponytail: 保持选项挂载并隐藏未匹配项，避免 Radix 因选中项卸载而抢走搜索焦点。 */}
            {options.map((option) => {
              const isFilteredOut = !matchesSearch(option)

              return (
                <RadixSelect.Item
                  key={option.value}
                  value={toInternalValue(option.value)}
                  hidden={isFilteredOut}
                  disabled={option.disabled || isFilteredOut}
                  className={`relative ${isFilteredOut ? 'hidden' : 'flex'} min-h-9 cursor-pointer select-none items-center rounded-lg py-2 pl-3 pr-9 text-sm text-[var(--text-secondary)] outline-none transition-colors data-[disabled]:pointer-events-none data-[disabled]:opacity-40 data-[highlighted]:bg-[var(--bg-hover)] data-[highlighted]:text-[var(--text-primary)] data-[state=checked]:bg-[var(--accent-primary)]/10 data-[state=checked]:text-[var(--accent-primary)]`}
                >
                  <RadixSelect.ItemText>{option.label}</RadixSelect.ItemText>
                  <RadixSelect.ItemIndicator className="absolute right-3 inline-flex items-center">
                    <Check className="h-4 w-4" />
                  </RadixSelect.ItemIndicator>
                </RadixSelect.Item>
              )
            })}
            {!hasMatchingOption && (
              <div className="px-3 py-8 text-center text-sm text-[var(--text-muted)]">
                未找到匹配项
              </div>
            )}
          </RadixSelect.Viewport>

        </RadixSelect.Content>
      </RadixSelect.Portal>
    </RadixSelect.Root>
  )
}
