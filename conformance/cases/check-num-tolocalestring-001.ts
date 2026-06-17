// `n.toLocaleString()` en-US default — `,` group sep / `.` decimal.
// bun / Node on macOS report en-US as the host DefaultLocale, so
// the canonical comma-grouped form is what test262 + user programs
// expect. Locale-arg variants are a separate Intl substrate item.
console.log((1000000).toLocaleString())
console.log((0).toLocaleString())
console.log((-1000).toLocaleString())
console.log((1234.5).toLocaleString())
console.log((1234567.89).toLocaleString())
console.log((100).toLocaleString())
console.log((1000).toLocaleString())
console.log((12345).toLocaleString())
console.log((1234567890).toLocaleString())
console.log((-1234567).toLocaleString())

// Edge cases — single digit / no grouping.
console.log((1).toLocaleString())
console.log((9).toLocaleString())
console.log((99).toLocaleString())
console.log((999).toLocaleString())
console.log((-999).toLocaleString())
console.log((1000).toLocaleString().length)

// Locale arg is ignored (tr's subset is en-US only).
console.log((1000).toLocaleString('en-US'))
