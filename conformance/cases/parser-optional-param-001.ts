// V3-18 wedge — TS optional parameter syntax `name?: T` on
// function declarations, class methods, and arrow fns. Per TS
// spec §3.9.2.4 the `?` lets the call site omit the arg; subset
// models it as Nullable<T> since real Type::Undefined substrate
// (Phase D) isn't yet in place. Pre-fix tora's parser bailed
// with 'expected `,` or `)`, got Question'.
//
// Subset limitation: caller must explicitly pass `null` to
// express "omitted". Bare `f()` for an optional-only signature
// isn't yet permitted (requires real undefined).

function id(s?: string): string | null { return s }
console.log(id("hi"))                  // hi
console.log(id(null))                  // null

function isAbsent(x?: string): boolean { return x === null }
console.log(isAbsent("hi"))            // false
console.log(isAbsent(null))            // true

// Ternary join over `T` and `Nullable<T>` — the `!` postfix
// strips Nullable<T> → T inside the else branch.
function greet(name?: string): string {
  return name === null ? "anon" : name!
}
console.log(greet("alice"))            // alice
console.log(greet(null))               // anon

// Arrow fn optional param with explicit return type that
// contains `| null` (requires arrow-fn lookahead to skip past
// the union suffix).
let lookup = (key?: string): string | null => key
console.log(lookup("x"))               // x
console.log(lookup(null))              // null
