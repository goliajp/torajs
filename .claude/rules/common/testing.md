# Testing Requirements

## Minimum Test Coverage: 80%

Test types (all required where applicable):
1. **Unit Tests** — individual functions, utilities, components
2. **Integration Tests** — API endpoints, database operations, module boundaries
3. **E2E Tests** — critical user flows (framework chosen per language)

## Test-Driven Development

Mandatory workflow:
1. Write test first (RED)
2. Run test — it must FAIL
3. Write minimal implementation (GREEN)
4. Run test — it must PASS
5. Refactor while green
6. Verify coverage (80%+)

## What to Test / What to Skip

**Test**:
- pure logic, algorithms, data transformations, error mapping
- parsers, validators, serializers
- state transitions, reducers, finite-state machines
- security boundaries (auth, authorization, input validation)
- edge cases: empty input, boundary values, error paths

**Skip**:
- glue code, framework wiring, dependency injection setup
- trivial getters/setters, simple re-exports, pure type definitions
- auto-generated code
- code whose only job is to call a framework primitive

**Never mock external dependencies** — if a test needs a mock for the database, HTTP client, or filesystem, the code under test is probably over-abstracted. Prefer integration tests against real dependencies (or test containers). Under-testing > over-testing; a brittle mock-heavy test suite is worse than missing tests.

## Troubleshooting Test Failures

1. Check test isolation (state leaking between tests)
2. Verify mocks are correct and in sync with real behavior
3. Fix the implementation, not the test — unless the test itself is wrong
