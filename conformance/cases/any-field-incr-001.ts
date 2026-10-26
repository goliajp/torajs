// ++ / -- on a class instance's `any` field: the slot holds a boxed
// value, so the step runs ToNumeric once and adds in the operand's own
// numeric domain (a double must not step by one ulp)
class B {
  a: any = 1
  k = 1
  bump() { return this.a++ }
  pre() { return ++this.a }
}
const b = new B()
b.a = b.k
b.a++
console.log(b.a)
b.a = 1.5
++b.a
console.log(b.a)
b.a = b.k + 0.5
console.log(b.bump(), b.a)
console.log(b.pre(), b.a)
b.a = "5"
const o1 = b.a++
console.log(o1, typeof o1, b.a)
b.a = "x"
b.a--
console.log(b.a)
b.a = 10n
b.a++
console.log(b.a, typeof b.a)
console.log(--b.a, b.a)
b.a = { valueOf() { return 41 } }
console.log(b.a++, b.a)
b.a = { valueOf() { b.a = 100; return 7 } }
console.log(b.a++, b.a)
b.a = { valueOf() { throw new Error("boom") } }
try { b.a++ } catch (e: any) { console.log("caught", e.message, typeof b.a) }
b.a = undefined
b.a++
console.log(b.a)
b.a = null
console.log(++b.a)
b.a = true
b.a--
console.log(b.a)
let s = 0
for (b.a = b.k / 2; b.a < 5; b.a++) s += b.a
console.log(s)

// a frozen instance refuses the store after the ToNumeric ran
const c = new B()
c.a = 2.5
Object.freeze(c)
try { c.a++; console.log("no throw", c.a) } catch (e: any) { console.log("frozen", e instanceof TypeError, c.a) }
const d = new B()
d.a = { valueOf() { Object.freeze(d); return 3 } }
try { d.a++; console.log("no throw") } catch (e: any) { console.log("frozen in valueOf", e instanceof TypeError, typeof d.a) }
const e = new B()
e.a = { v: 7, valueOf() { e.a = 100; return this.v } }
console.log("self store", e.a++, e.a)
