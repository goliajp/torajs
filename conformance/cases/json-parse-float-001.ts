// T-02 (v0.3.0) — JSON.parse f64 path. JS spec Number is f64, so
// `let v: number = JSON.parse(text)` must hold a fractional value
// without truncation. tr's wider i64-default rule for `number`
// doesn't apply to JSON.parse caller sites because the JSON grammar
// carries no compile-time hint of integer-vs-decimal.
let a: number = JSON.parse("1.5")
console.log(a)

// Integer-valued JSON in the same `: number` slot still prints
// without trailing ".0" — bun-parity (Number.toString on an integer-
// valued f64 omits the decimal).
let b: number = JSON.parse("42")
console.log(b)

// Negative fractional + leading sign + zero decimal — round-trip.
let c: number = JSON.parse("-3.14")
console.log(c)

// Scientific notation inside the JSON text.
let d: number = JSON.parse("2.5e3")
console.log(d)

// Explicit `: i64` opts back into the integer parser (advanced /
// performance path; truncates fractional input the same way
// parseInt would).
let e: i64 = JSON.parse("7")
console.log(e)

// Explicit `: f64` was already wired pre-T-02; verify still green.
let f: f64 = JSON.parse("0.125")
console.log(f)
