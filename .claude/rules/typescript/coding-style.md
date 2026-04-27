---
paths:
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.js"
  - "**/*.jsx"
---
# TypeScript/JavaScript Coding Style

> This file extends [common/coding-style.md](../common/coding-style.md) with TypeScript/JavaScript specific content.

## Type System (CRITICAL)

- **`type` only** — NEVER use `interface` or `enum`
- Use union types instead of enums:
  ```typescript
  // WRONG
  enum Status { Active, Inactive }
  interface User { name: string }

  // CORRECT
  type Status = 'active' | 'inactive'
  type User = { name: string }
  ```

## Import Style

- Separate `import type` from value imports — never inline:
  ```typescript
  // WRONG
  import { type Foo, bar } from 'lib'

  // CORRECT
  import type { Foo } from 'lib'
  import { bar } from 'lib'
  ```
- Path alias: `@/` maps to `./src`

## Components

- Function declarations for named components (not arrow functions):
  ```typescript
  // WRONG
  const MyComponent = () => { ... }

  // CORRECT
  function MyComponent() { ... }
  ```

## Nullish Coalescing

- Always use `??` over `||` for defaults
- Beware: useQuery `data` can be `null`, not just `undefined`:
  ```typescript
  // WRONG — won't catch null
  const { data: items = [] } = useItems()

  // CORRECT
  const { data } = useItems()
  const items = data ?? []
  ```

## Immutability

Use spread operator for immutable updates:

```typescript
// WRONG
user.name = name

// CORRECT
return { ...user, name }
```

## Runtime & Package Manager

- **bun** is the default runtime and package manager — never use npm/yarn/pnpm
- `bun install`, `bun run`, `bun test`, `bunx` for all operations

## Formatting

- **Prettier**: single quotes, no semicolons, 100 char width, trailing comma es5
- **ESLint**: perfectionist import sorting, unused-imports plugin

## Error Handling

- No silent failures — never leave promises floating without `await` or `.catch()`
- Use `.catch(logger.warn)` for non-critical async calls

## Environment Variables

Load secrets and config from environment variables. Fail fast at startup if a required variable is missing — never paper over it with defaults:

```typescript
// WRONG — hardcoded secret
const apiKey = 'sk-proj-xxxxx'

// WRONG — silent fallback to undefined
const apiKey = process.env.OPENAI_API_KEY

// CORRECT — explicit fail-fast
const apiKey = process.env.OPENAI_API_KEY
if (!apiKey) throw new Error('OPENAI_API_KEY not configured')
```

Document required variables in `.env.example`; never commit `.env`.

## Input Validation

Use Zod for schema-based validation:

```typescript
import { z } from 'zod'

const schema = z.object({
  email: z.string().email(),
  age: z.number().int().min(0).max(150),
})

const validated = schema.parse(input)
```

## Console.log

- No `console.log` statements in production code
- Use structured logger per module instead
