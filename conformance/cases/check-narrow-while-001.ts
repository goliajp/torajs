// V3-18 wedge — flow narrowing on while-loop conditions of
// the form `while (<ident> !== null) { ... }`. Per TS spec
// the narrowing applies inside the loop body. Pre-fix tora
// rejected `o.v` inside the loop body as 'no member .v on
// Nullable<...>'.
//
// Implementation: in the Stmt::While handler, reuse the
// collect_null_narrow / apply_narrow / restore_narrow helpers
// from the if-narrow path. Subset limitation: narrowing is
// SKIPPED when the loop body reassigns the bound name —
// `while (cur !== null) { cur = cur.next }` would re-narrow
// each iteration but the RHS `cur.next` is still
// `Nullable<T>`, conflicting with the narrowed `T` LHS. Use
// the postfix `!` workaround (`cur = cur!.next`) in those
// cases — the next iteration's cond re-checks anyway.

type O = { v: number }
let o: O | null = { v: 42 }
while (o !== null) {
  console.log(o.v)                     // narrowed access
  break
}

// while != null (loose).
let p: O | null = { v: 99 }
while (p != null) {
  console.log(p.v)
  break
}

// Common pattern: poll-style loop without reassigning the
// bound name (the loop exits via break or external signal).
let attempt: O | null = { v: 7 }
let count = 0
while (attempt !== null) {
  count = count + 1
  if (count >= 3) break
  console.log(attempt.v)               // narrowed
}
console.log("done after", count)
