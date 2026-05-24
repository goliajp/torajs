# torajs-meta performance budgets

`torajs-meta`'s three concerns sit on **warm paths**:

| Path | Hot/Warm | Notes |
| --- | --- | --- |
| `fnprops` get / set | Warm | Per fn-as-object property access. |
| `classmeta` instanceof check | Warm | Per `x instanceof C` user-code op. |
| `reflect` ops | Cold | Per Object.* meta call; rare on bench corpus. |

## Budgets (informational; no perf-gate tests)

| Path | Budget | Notes |
| --- | ---: | --- |
| fnprops get (hit) | < 50 ns | One hashmap probe + Str alloc. |
| fnprops get (miss) | < 30 ns | Hashmap probe → undefined. |
| classmeta class-name lookup | < 20 ns | Indexed array lookup by class_tag. |
| classmeta instanceof (depth ≤ 4) | < 100 ns | Prototype chain walk. |
| reflect getOwnPropertyDescriptor | < 200 ns | Property table probe + Descriptor heap alloc. |

## Memory budget

- `fnprops` hashmap: O(N) where N = count of functions that have
  had a property set on them. Empty for the bench corpus (no fn-
  as-object usage).
- `classmeta` registries: O(C) where C = class count at codegen.
  ~50-100 entries for non-trivial programs.

## What's NOT budgeted

- **High-cardinality fnprops**: would need to revisit the hashmap
  shape (probably resize to a more memory-efficient structure).
  Not benched today.
- **Concurrent meta-table access**: torajs is single-threaded.
- **Decorator metadata**: not yet surfaced.
