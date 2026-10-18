// A class-typed field of an object type, reached while an object
// through `any` is copied into that type: the field's value is copied
// into the class like a value cast into the class directly, so one
// owning a property under a method name of the class (an object
// literal's method, a `JSON.parse` result's data property) is refused
// with a TypeError, and one owning none is copied. bun keeps the
// objects as they are; the refused lines differ and are checked
// against the .expected.
class W {
  a = 1
  m(): number {
    return 10
  }
}
type N = { inner: { w: W } }
function giveN(v: any): N {
  return v
}
const own: any = { a: 2, m() { return 20 } }
const other: any = { a: 5 }
try {
  console.log(giveN({ inner: { w: own } }).inner.w.a)
} catch (e) {
  console.log(e instanceof TypeError)
}
console.log(giveN({ inner: { w: other } }).inner.w.a)
const parsed: any = JSON.parse('{"inner":{"w":{"a":7,"m":1}}}')
try {
  console.log(giveN(parsed).inner.w.a)
} catch (e) {
  console.log(e instanceof TypeError)
}
