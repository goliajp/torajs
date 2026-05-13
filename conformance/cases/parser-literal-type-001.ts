// V3-18 wedge — TS literal type annotations:
//   type Mode = "dev" | "prod" | "test"
//   type Bit = 0 | 1
//   type Always = true
// Per TS spec §3.2.10 a literal type carries exactly one
// constant value as its inhabitant; unions of literals are
// the canonical TS shape for finite enumerations. Pre-fix
// tora's parse_type_ann bailed at any literal token in
// type-ann position with 'expected type name, got
// String(...) / Number(...) / True / False'.
//
// Implementation: parse_type_ann's entry detects a literal
// (String / Number / True / False) at the type position,
// consumes it + any '| <literal>' chain of the same kind,
// and returns the widened primitive name ("string" /
// "number" / "boolean"). Subset limitation: the literal
// constraint is NOT enforced — `let m: Mode = "garbage"`
// typechecks, since tora widens to `string` (matches the
// pragmatic erasure pattern most TS subset compilers
// adopt). Cross-kind unions (e.g. `string | number`) still
// require general-union substrate (Phase later).

type Mode = "dev" | "prod" | "test"
let m: Mode = "test"
console.log(m)                          // test

type Bit = 0 | 1
let b: Bit = 0
console.log(b)                          // 0

type Always = true
let a: Always = true
console.log(a)                          // true

// Inline-obj field with literal-union type.
type Log = { level: "info" | "warn" | "error"; msg: string }
let l: Log = { level: "info", msg: "hi" }
console.log(l.level, l.msg)             // info hi

// Function with literal-union param and return.
function level(m: Mode): string { return m }
console.log(level("prod"))              // prod
console.log(level("dev"))               // dev

// Single literal (no union).
type Tag = "user"
let t: Tag = "user"
console.log(t)                          // user
