import { useFonts } from '@goliapkg/gds'
import { useSetThemeDensity, useSetThemeMotion, useThemeEffect } from '@goliapkg/gds/systems'
import { useEffect } from 'react'
import { Link, Outlet, useLocation } from 'react-router'

import { ThemeToggle } from './components/theme-toggle'

const NAV = [{ label: 'Home', path: '/' }]

export function AppLayout() {
  useThemeEffect()
  useFonts()

  const setDensity = useSetThemeDensity()
  const setMotion = useSetThemeMotion()
  useEffect(() => {
    setDensity('default')
    setMotion('full')
  }, [setDensity, setMotion])

  const location = useLocation()

  const isActive = (path: string) =>
    path === '/' ? location.pathname === '/' : location.pathname.startsWith(path)

  return (
    <div className="flex h-full flex-col">
      <header className="border-border bg-bg/80 flex h-12 shrink-0 items-center justify-between border-b px-6 backdrop-blur-xl">
        <div className="flex items-center gap-6">
          <Link className="text-fg flex items-center gap-2 text-sm font-semibold" to="/">
            torajs
          </Link>
          <nav className="flex items-center gap-1">
            {NAV.map((item) => (
              <Link
                className={`rounded-md px-3 py-1.5 text-sm transition-colors ${
                  isActive(item.path)
                    ? 'bg-accent/10 text-accent'
                    : 'text-fg-muted hover:bg-bg-tertiary hover:text-fg'
                }`}
                key={item.path}
                to={item.path}
              >
                {item.label}
              </Link>
            ))}
          </nav>
        </div>
        <ThemeToggle />
      </header>

      <main className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-4xl px-6 py-8">
          <Outlet />
        </div>
      </main>

      <footer className="border-border text-fg-muted flex h-8 shrink-0 items-center justify-center border-t bg-white/5 text-xs backdrop-blur-sm">
        torajs.com
      </footer>
    </div>
  )
}
