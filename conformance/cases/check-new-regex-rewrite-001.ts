// P1 — `new RegExp(pattern?, flags?)` per ES spec §22.2.3.1.
// Rewrite shapes with constant-string args to the equivalent
// regex literal `Expr::Regex { pattern, flags }`. Pre-fix tora
// bailed at 'unknown identifier `__new_RegExp`' since the
// class-lowering desugar synthesizes `__new_C` factories for
// user classes only — built-in RegExp has no factory. Test262
// uses `new RegExp()` / `new RegExp("pat")` / `new RegExp(p, f)`
// pervasively (~18+ cases blocked across the broader sample
// under built-ins/Array/* and built-ins/RegExp/*).
//
// Implementation: ast.rs `desugar_builtin_new` Pass — for each
// `Expr::New { class_name = "RegExp", args }`:
//   - 0 args               → /(?:)/
//   - 1 string-literal arg → /<arg>/
//   - 2 string-literal args → /<arg0>/<arg1>
// Dynamic-arg shapes (`new RegExp(varRef)`) keep the unknown-
// factory error — the regex pattern must be statically known at
// lower time so the C-side compiled regex can be embedded.

// 0-arg form — empty regex matches every string.
let r1 = new RegExp()
console.log(r1.test(""))                     // true
console.log(r1.test("hello"))                // true

// 1-string-arg form.
let r2 = new RegExp("foo")
console.log(r2.test("foo bar"))              // true
console.log(r2.test("baz"))                  // false

// 2-string-arg form (pattern + flags).
let r3 = new RegExp("hello", "i")
console.log(r3.test("HELLO"))                // true (case-insensitive)
console.log(r3.test("nope"))                 // false

// Combined with .exec.
let r4 = new RegExp("(\\d+)")
let m = r4.exec("abc 42 def")
console.log(m[0])                            // 42
console.log(m[1])                            // 42
