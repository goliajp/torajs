// ES §22.1.2.2 — `Array.isArray(v)` answers per the runtime value's
// heap tag, not the static type. Pre-fix tr resolved purely at compile
// time (`Type::Arr(_)` → true; anything else → false), so an Any-typed
// binding that at runtime holds an Array returned false. Fix routes
// `Type::Any` arg through `__torajs_any_is_arr` (NaN-box unbox → tag
// gate ANY_HEAP → heap-header type_tag == TAG_ARR).

const a: any = [1, 2, 3]
console.log(Array.isArray(a))                // true (was false)

const b: any = { x: 1 }
console.log(Array.isArray(b))                // false

const c: any = 42
console.log(Array.isArray(c))                // false

const d: any = "hello"
console.log(Array.isArray(d))                // false

const e: any = null
console.log(Array.isArray(e))                // false

const f: any = undefined
console.log(Array.isArray(f))                // false

const g: any = true
console.log(Array.isArray(g))                // false

// Static fast paths unchanged
console.log(Array.isArray([1, 2]))           // true (typed Array literal)
const typed: number[] = [1, 2, 3]
console.log(Array.isArray(typed))            // true (Type::Arr fast path)
console.log(Array.isArray(42))               // false (Type::Number static)
console.log(Array.isArray("foo"))            // false (Type::String static)

// Any-typed function parameter — the typeof pattern from JS
function classify(x: any): string {
  if (Array.isArray(x)) return "arr"
  if (typeof x === "number") return "num"
  if (typeof x === "string") return "str"
  return "other"
}
console.log(classify([1, 2]))                // "arr" (was "other")
console.log(classify(1))                     // "num"
console.log(classify("a"))                   // "str"
console.log(classify({}))                    // "other"
