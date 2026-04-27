---
paths:
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.js"
  - "**/*.jsx"
---
# TypeScript/JavaScript Patterns

## API Definition Pattern

Group related API calls as plain objects (not classes):

```typescript
export const usersApi = {
  async list(params?: ListOptions) { ... },
  async get(id: string) { ... },
  async create(data: Partial<User>) { ... },
  async update(id: string, data: Partial<User>) { ... },
  async delete(id: string) { ... },
}
```

## React Query Key Factory

```typescript
export const userKeys = {
  all: ['users'] as const,
  lists: () => [...userKeys.all, 'list'] as const,
  list: (params?: ListOptions) => [...userKeys.lists(), params] as const,
  details: () => [...userKeys.all, 'detail'] as const,
  detail: (id: string) => [...userKeys.details(), id] as const,
}
```

## Data-Fetching Hook Pattern

```typescript
export function useUsers<TData = User[]>(
  params?: ListOptions,
  options?: Omit<UseQueryOptions<User[], Error, TData>, 'queryFn' | 'queryKey'>,
) {
  const { isAuthenticated } = useSession()
  return useQuery({
    enabled: isAuthenticated,
    queryKey: userKeys.list(params),
    staleTime: 1000 * 60 * 5,
    queryFn: () => usersApi.list(params),
    ...options,
  })
}
```

## Routing (CRITICAL)

**ALWAYS use `react-router` for React web apps.** The URL is the single source of truth for navigation and page state.

### Core Principles

- Every distinct view MUST have its own URL path — if you can see it, you can link to it
- Browser back/forward and direct URL entry MUST work correctly
- Use `createBrowserRouter` with `<RouterProvider>` (React Router v7 data API)
- Use `<Link>` / `useNavigate()` for navigation — NEVER use atoms, `useState`, or custom routing

### Route Registration

Register all routes in one place (`main.tsx`), flat and explicit. No lazy magic, no file-system conventions:

```typescript
const router = createBrowserRouter([
  {
    element: <AppLayout />,
    path: '/',
    children: [
      { index: true, element: <HomeView /> },
      { path: 'users', element: <UsersView /> },
      { path: 'users/:id', element: <UserDetailView /> },
      { path: 'settings', element: <SettingsView /> },
      { element: <Navigate replace to="/" />, path: '*' },
    ],
  },
])
```

Rules:
- One `children` array, no nesting beyond layout → pages
- Catch-all `*` route at the end, redirect to home
- Dynamic segments use `:param` — read with `useParams()`

### URL as State

Maximize what the URL expresses. A user sharing a URL should land on the same view with the same state:

| State type | Where to put it | Example |
|------------|----------------|---------|
| Which page | path segment | `/users`, `/settings` |
| Which item | path param | `/users/42` |
| Active tab | query param | `/users?tab=inactive` |
| Search/filter | query param | `/users?q=john&role=admin` |
| Sort order | query param | `/users?sort=name&dir=asc` |
| Pagination | query param | `/users?page=3` |
| Modal open | query param | `/users?edit=42` |
| Anchor position | hash | `/docs#installation` |

```typescript
// WRONG — tab state in useState, lost on refresh
const [tab, setTab] = useState('active')

// CORRECT — tab state in URL, survives refresh and sharing
const [params, setParams] = useSearchParams()
const tab = params.get('tab') ?? 'active'
const setTab = (t: string) => setParams({ tab: t })
```

### Hash Routes

Use hash (`#`) for in-page anchors only — scrolling to a section within a single page view. Do NOT use hash-based routing (`HashRouter`) for page navigation:

```typescript
// CORRECT — hash for in-page scroll targets
<a href="#pricing">Jump to Pricing</a>
<section id="pricing">...</section>

// CORRECT — long-form page with multiple sections
{ path: 'docs', element: <DocsView /> }  // sections addressed via #installation, #api, #faq

// WRONG — hash for page navigation
<a href="#/users">Users</a>  // use path-based routing instead
```

### Subpath Deployment

When deploying under a subpath (e.g., `/starters/web/`):
- Set `base` in `vite.config.ts`: `base: '/starters/web/'`
- Set `basename` in `createBrowserRouter`: `{ basename: '/starters/web' }`
- Both must match, or assets and routing will break

### Anti-Patterns

```typescript
// WRONG — atom-based routing
const routeAtom = atom<string>('overview')

// WRONG — useState routing
const [page, setPage] = useState('home')

// WRONG — conditional rendering as routing
{page === 'home' && <Home />}
{page === 'about' && <About />}

// WRONG — window.location for SPA navigation
window.location.href = '/users'

// CORRECT — react-router everywhere
<Link to="/users">Users</Link>
navigate('/users')
```

## State Management

- **React Query** for server state
- **Jotai** atoms for ephemeral client-only state (modals, local toggles) — NOT for navigation (see Routing section above)
- Use `useSetAtom(atom)` when only the setter is needed
- For React Native projects, pair Jotai with MMKV for persistence; on the web, prefer URL + localStorage over MMKV

## API Response Format

```typescript
type ApiResponse<T> = {
  success: boolean
  data?: T
  error?: string
  meta?: {
    total: number
    page: number
    limit: number
  }
}
```

## Custom Hooks

```typescript
export function useDebounce<T>(value: T, delay: number): T {
  const [debouncedValue, setDebouncedValue] = useState<T>(value)

  useEffect(() => {
    const handler = setTimeout(() => setDebouncedValue(value), delay)
    return () => clearTimeout(handler)
  }, [value, delay])

  return debouncedValue
}
```
