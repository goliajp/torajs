// Spec §13.15.3 — `String + Any` (and reverse) returns String, not
// Any. Pre-fix the BinOp::Add arm in check.rs fell through to the
// catch-all Any branch, forcing callers to add a redundant `as
// string` cast for the common pattern below:
//
//   catch (e: any) { throw new Error('prefix: ' + e.message) }
//
// where `e.message` is Type::Any. `new Error(Any)` then rejected
// with `argument 0: expected String, got Any`. Spec
// ApplyStringOrNumericBinaryOperator routes one-String operands
// through StringConcat (ToString on the non-String side), so the
// result is always a String.

// Concat-then-pass-to-Error — the original silent-wrong site.
try {
  try {
    throw new Error('inner')
  } catch (e: any) {
    throw new Error('rethrow: ' + e.message)
  }
} catch (e: any) {
  console.log(e.message)
}

// Reverse order — `Any + String`.
const obj: any = { name: 'world' }
const greeting = obj.name + '!'
console.log(greeting)

// Chained — both `Str + Any` and `Any + Str` in a single expression.
const banner = '[' + obj.name + ']'
console.log(banner)
