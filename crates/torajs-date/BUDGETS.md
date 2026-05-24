# torajs-date performance budgets

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| Construction | Warm | Per `new Date(...)`. |
| UTC getter | Warm | Per `getUTCX()`. Pure arithmetic via civil_from_days. |
| Local getter | Warm | Per `getX()`. Goes through libc `localtime_r`. |
| `toISOString` | Warm | Per `.toISOString()`. ~30-char Str alloc + format. |
| `parse` | Cold | Per `Date.parse(s)`. State machine over the ISO 8601 subset. |

## Budgets

| Path | Budget | Notes |
| --- | ---: | --- |
| `civil_from_days` | < 30 ns | Howard Hinnant's algorithm — branch-light, integer-only. |
| `getUTCFullYear` / `_Month` / `_Date` etc. | < 50 ns | civil_from_days + bit-pick. |
| `getFullYear` / `_Month` / etc. (local) | < 500 ns | libc localtime_r dominates. |
| `toISOString` | < 200 ns | civil + format-into-Str. Single Str alloc. |
| `parse` ISO 8601 | < 1 µs | State machine over ~25-char input. |

## What's NOT budgeted

- **libc `localtime_r` / `mktime` per-call cost**: OS-dependent;
  typically ~300-500 ns on modern aarch64 macOS. Out of our scope.
- **Far-future / far-past dates**: civil_from_days handles ±290M
  years correctly but the libc localtime can fail or saturate for
  extreme inputs. Edge cases.
- **DST transition correctness**: defers to libc localtime; we
  don't second-guess the kernel's tzinfo.
