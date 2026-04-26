import './index.css'

import { loadPersistedTheme, resolveThemeCssVars } from '@goliapkg/gds/systems'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createBrowserRouter, Navigate, RouterProvider } from 'react-router'

import { AppLayout } from './app'
import { HomeView } from './views/home'

// pre-render theme to avoid FOUC
const saved = loadPersistedTheme()
if (saved) {
  const mode =
    saved.mode === 'system'
      ? window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      : saved.mode
  const vars = resolveThemeCssVars(saved, mode as 'dark' | 'light')
  const root = document.documentElement
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v as string)
  root.dataset.theme = mode
}

const router = createBrowserRouter([
  {
    children: [
      { element: <HomeView />, index: true },
      { element: <Navigate replace to="/" />, path: '*' },
    ],
    element: <AppLayout />,
    path: '/',
  },
])

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchInterval: 60_000,
      retry: 1,
      staleTime: 30_000,
    },
  },
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>
)
