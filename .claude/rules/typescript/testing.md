---
paths:
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.js"
  - "**/*.jsx"
---
# TypeScript/JavaScript Testing

> Extends `common/testing.md` with TypeScript/JavaScript-specific content. Read the common file first for coverage targets, TDD workflow, and "what to test / never mock" guidance.

## Test Frameworks

- **Vitest** for unit tests (`.test.ts`) — pure logic, utilities, parsers, state atoms
- **Jest** (jest-expo for React Native) for component tests (`.test.tsx`) — anything that needs React context or native module mocks
- **Playwright** for end-to-end web tests; **Maestro** for mobile E2E
- Never mix runners in one file: `.test.ts` cannot import React components; `.test.tsx` should not test pure logic

## Test Placement

Co-located with source: `foo.ts` → `foo.test.ts` in the same directory. Remove tests when the code they cover is deleted or substantially rewritten.

## Coverage

Istanbul provider; keep separate reports for unit and component runs and merge before reporting (`coverage/unit/`, `coverage/integration/`, `coverage/merged/`).
