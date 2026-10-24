// Generators without a return-type annotation whose yields and valued
// returns agree on one type: the values, the resume channel, the
// completion value and the closes all read as they do untyped.
function* nums() { yield 1; yield 2; yield 3 }
let s = 0
for (const v of nums()) s = s + v
console.log(s)

// next(v) still takes any value
function* sent() { const a = yield 1; console.log("got", a); yield 2 }
const it = sent()
console.log(it.next().value)
console.log(it.next("a").value)
console.log(it.next().done)

// a yield used as a value yields what was sent back
function* echo() { let x = yield 10; while (x !== undefined) { x = yield x } }
const ec = echo()
console.log(ec.next().value, ec.next(4).value, ec.next(9).value, ec.next().done)

// yields and returns of different types stay untyped
function* mixedRet() { yield 1; return "s" }
const m = mixedRet()
console.log(m.next().value, m.next().value, m.next().value)
function* multi() { yield 1; yield "two"; yield true }
for (const v of multi()) console.log(typeof v, v)

// completion values
function* endRet() { yield 1; return 5 }
const er = endRet()
console.log(er.next().value, er.next().value, er.next().value)
function* bare() { yield 1; return }
const b = bare()
console.log(b.next().value, b.next().value)
const n1 = nums()
n1.next(); n1.next(); n1.next()
const done = n1.next()
console.log(done.value, done.done, typeof done.value, JSON.stringify(done))
const viaAny: any = nums()
viaAny.next(); viaAny.next(); viaAny.next()
const end: any = viaAny.next()
console.log(end.value, end.done)

// other yield types
function* strs(n: number) { let i = 0; while (i < n) { yield "s" + i; i = i + 1 } }
console.log([...strs(3)].join(","))
function* objs() { yield { a: 1, b: "x" }; yield { a: 2, b: "y" } }
for (const o of objs()) console.log(o.a, o.b)
function* arrs() { yield [1, 2]; yield [3] }
for (const a of arrs()) console.log(a.length, a[0])
function* inner() { yield 1; yield 2; return 3 }
function* outer() { const r = yield* inner(); yield r }
for (const v of outer()) console.log(v)
const gx = function* () { yield 4; yield 5 }
let t = 0
for (const v of gx()) t += v
console.log(t)
class K { *items() { yield 1; yield 2 } }
for (const v of new K().items()) console.log(v)
const [d1, d2] = nums()
console.log(d1, d2, Array.from(nums()).length, JSON.stringify([...nums()]))
