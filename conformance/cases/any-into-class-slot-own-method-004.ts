// A class-typed field of an object type, reached through an object
// handed over through `any`: the object and its field's value are held
// as they are, whether the value owns a property under a method name of
// the class (an object literal's method, a `JSON.parse` result's data
// property) or none.
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
