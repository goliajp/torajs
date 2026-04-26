import type { ThemeMode } from '@goliapkg/gds/systems'

import { themeAtom } from '@goliapkg/gds/systems'
import { atom, getDefaultStore, useAtomValue } from 'jotai'

const store = getDefaultStore()

const themeModeAtom = atom(
  (get) => get(themeAtom).mode,
  (get, set, mode: ThemeMode) => {
    set(themeAtom, { ...get(themeAtom), mode })
  }
)

function setThemeMode(mode: ThemeMode): void {
  store.set(themeModeAtom, mode)
}

const modes: { icon: React.ReactNode; label: string; value: ThemeMode }[] = [
  {
    icon: (
      <svg className="h-3.5 w-3.5" fill="currentColor" viewBox="0 0 20 20">
        <path d="M10 2a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5A.75.75 0 0 1 10 2ZM10 15a5 5 0 1 0 0-10 5 5 0 0 0 0 10ZM10 17a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5A.75.75 0 0 1 10 17ZM17 10a.75.75 0 0 1 .75-.75h1.5a.75.75 0 0 1 0 1.5h-1.5A.75.75 0 0 1 17 10ZM2 10a.75.75 0 0 1 .75-.75h1.5a.75.75 0 0 1 0 1.5h-1.5A.75.75 0 0 1 2 10ZM15.657 15.657a.75.75 0 0 1 0-1.06l1.06-1.061a.75.75 0 1 1 1.061 1.06l-1.06 1.061a.75.75 0 0 1-1.061 0ZM3.283 4.343a.75.75 0 0 1 0-1.06l1.06-1.061a.75.75 0 1 1 1.061 1.06l-1.06 1.061a.75.75 0 0 1-1.061 0ZM15.657 4.343a.75.75 0 0 1 1.06-1.06l1.061 1.06a.75.75 0 0 1-1.06 1.061l-1.061-1.06ZM3.283 15.657a.75.75 0 0 1 1.06-1.06l1.061 1.06a.75.75 0 1 1-1.06 1.061l-1.061-1.06Z" />
      </svg>
    ),
    label: 'Light',
    value: 'light',
  },
  {
    icon: (
      <svg className="h-3.5 w-3.5" fill="currentColor" viewBox="0 0 20 20">
        <path
          clipRule="evenodd"
          d="M2 4.25A2.25 2.25 0 0 1 4.25 2h11.5A2.25 2.25 0 0 1 18 4.25v8.5A2.25 2.25 0 0 1 15.75 15h-3.105a3.501 3.501 0 0 0 1.1 1.677A.75.75 0 0 1 13.26 18H6.74a.75.75 0 0 1-.484-1.323A3.501 3.501 0 0 0 7.355 15H4.25A2.25 2.25 0 0 1 2 12.75v-8.5Zm1.5 0a.75.75 0 0 1 .75-.75h11.5a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-.75.75H4.25a.75.75 0 0 1-.75-.75v-7.5Z"
          fillRule="evenodd"
        />
      </svg>
    ),
    label: 'System',
    value: 'system',
  },
  {
    icon: (
      <svg className="h-3.5 w-3.5" fill="currentColor" viewBox="0 0 20 20">
        <path
          clipRule="evenodd"
          d="M7.455 2.004a.75.75 0 0 1 .26.77 7 7 0 0 0 9.958 7.967.75.75 0 0 1 1.067.853A8.5 8.5 0 1 1 6.647 1.921a.75.75 0 0 1 .808.083Z"
          fillRule="evenodd"
        />
      </svg>
    ),
    label: 'Dark',
    value: 'dark',
  },
]

export function ThemeToggle() {
  const current = useAtomValue(themeModeAtom)
  return (
    <div className="bg-bg-tertiary flex items-center rounded-full p-0.5">
      {modes.map((mode) => (
        <button
          className={`cursor-pointer rounded-full p-1 transition-colors ${
            current === mode.value
              ? 'bg-bg text-fg shadow-sm'
              : 'text-fg-muted hover:text-fg-secondary'
          }`}
          key={mode.value}
          onClick={() => setThemeMode(mode.value)}
          title={mode.label}
        >
          {mode.icon}
        </button>
      ))}
    </div>
  )
}
