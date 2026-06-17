// ES §21.4.4.37 — `Date.prototype.toJSON(key?)` is defined as
// `this.toISOString()` for any finite Date. The optional `key`
// argument is silently ignored per spec. Pre-fix tr rejected the
// call at the type checker with "no member `.toJSON` on type Date".
// Fix wires it to the same runtime helper as `.toISOString` (the MVP
// non-finite short-circuit is independent substrate; the ms field
// already rejects non-finite ToNumber).

// Local-time constructor — same date in UTC depends on TZ; on the
// bun mini host (Asia/Tokyo) and the dev tr (UTC) the resulting
// toISOString output is determined by the constructor's local-time
// interpretation. We assert equality between `.toJSON()` and
// `.toISOString()` rather than a literal string so this stays TZ-
// portable across hosts.
const d = new Date(2024, 5, 15, 12, 30, 45)
console.log(d.toJSON() === d.toISOString())  // true

// epoch — both runtimes agree
const epoch = new Date(0)
console.log(epoch.toJSON())                   // "1970-01-01T00:00:00.000Z"
console.log(epoch.toISOString())              // "1970-01-01T00:00:00.000Z"
console.log(epoch.toJSON() === epoch.toISOString())  // true

// 2024 millennium anchor
const ms = new Date(1718442645000)
console.log(ms.toJSON())                      // "2024-06-15T08:30:45.000Z"
console.log(ms.toJSON() === ms.toISOString()) // true

// Negative epoch
const past = new Date(-1)
console.log(past.toJSON())                    // "1969-12-31T23:59:59.999Z"
console.log(past.toJSON() === past.toISOString())  // true
