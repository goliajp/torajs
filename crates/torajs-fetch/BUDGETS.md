# torajs-fetch performance budgets

`torajs-fetch` makes one HTTP call per `fetch(url)` user invocation
via libcurl-easy. Latency is dominated by **network RTT** + libcurl
setup; the Rust wrapper itself adds ~10-50 µs of overhead which is
irrelevant next to even the fastest network roundtrip (~1 ms locally,
~50-200 ms over the public internet).

## Path taxonomy

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `__torajs_fetch_sync` | Warm | Per `fetch(url)` user call. Network-dominated. |
| `__torajs_response_drop` | Warm | Paired with fetch_sync. Two free()s (body Str + Response block). |

## Budgets (Rust-side overhead only — network excluded)

| Path | Budget | Observed (dev, network excluded) | Notes |
| --- | ---: | ---: | --- |
| `__torajs_fetch_sync` setup | ≤ 200 µs | ~50 µs | curl_easy_init + curl_easy_setopt × N + write-fn registration. The curl_easy_perform call is network-dominated; not budgeted here. |
| `__torajs_response_drop` | ≤ 1 µs | ~200 ns | Two free()s + universal-header refcount-dec. |

## Network latency (informational only — out of scope)

| Path | Approx | Notes |
| --- | ---: | --- |
| Localhost (`http://127.0.0.1/...`) | ~1 ms | TCP + first-byte. |
| Same-region public (`https://example.com/...`) | ~50 ms | TLS handshake + first-byte. |
| Cross-region public | ~200 ms+ | Variable. |

## Allocation count per `fetch(url)`

- 1 × `Response` heap block (16 bytes header + 8 + body Str ptr).
- 1 × body `Str` heap block (16-byte header + N response body
  bytes).
- libcurl internal allocations (variable; tens of small allocs per
  call).

## What's NOT budgeted

- **Network RTT**: out of our control; user-facing.
- **TLS / DNS / connection-pooling**: libcurl's defaults; not
  tuned by torajs.
- **Body streaming**: full body materialized before return; no
  partial / incremental yield.
- **Connection reuse across calls**: each `fetch_sync` is a fresh
  `curl_easy_init`. A future polish step could share a `curl_easy*`
  handle pool for sequential same-host calls.
- **WASI fetch routing**: T-21.b will route through the browser
  fetch API; not in this crate's scope.
