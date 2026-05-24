# torajs-regex performance budgets

Regex performance depends on pattern + input pathology. Budgets here
target the common case (simple patterns on short-to-medium input);
adversarial backtrack patterns are outside scope.

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `RegExp` construct | Warm | One-time parse + compile per `new RegExp(...)`. |
| `regex.test(s)` | Warm/Hot | Per match attempt; loop tight. |
| `regex.exec(s)` | Warm/Hot | Same + capture-group extraction. |
| `s.replace(/pat/, ...)` | Warm | Per replace call. |
| `s.split(/pat/)` | Warm | Tokenize-with-regex. |

## Per-op budgets (common-case patterns, ASCII input)

| Op | Budget | Notes |
| --- | ---: | --- |
| Parse `/[a-z]+/` | < 5 µs | Recursive-descent over ~10 chars. |
| Compile to NFA | < 5 µs | ~10 NFA states. |
| `test()` on 64-byte input | < 200 ns | Pike VM ~64 steps. |
| `exec()` with 2 captures | < 500 ns | Same + capture-group copy. |

## Pathological patterns

Backtrack-heavy patterns like `/(a+)+b/` on `"aaaaaaaaaaa"` (no `b`)
can hit superlinear time. Pike VM is *NFA simulation*, not real
backtracking — it caps at O(n × m) for `n` input chars × `m` NFA
states, which is the upper bound. Adversarial patterns still hit
that bound but don't go superlinear like Perl-style backtracking.

## What's NOT budgeted

- JIT-compiled regex (would need a separate ship phase).
- Full ICU Unicode tables (only curated subset shipped).
- Very large pattern compilation (> 1000 chars).
