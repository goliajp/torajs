// §7.4.4 IteratorComplete step 1 — the IteratorResult must be an
// OBJECT. A next() answering a primitive value (string / symbol /
// number / undefined) is a TypeError, NOT a { done: undefined }
// read: tr's step driver judged "object" as "heap cell", which let
// a string result spin the drive loop forever (rotation 408 —
// sm/Iterator/prototype/reduce, exposed once the generic-tag proto
// alias made the reified next face resolvable).

class TestIterator extends Iterator {
  value: any
  constructor(value: any) { super(); this.value = value }
  next() { return this.value }
}
const sum = (x: any, y: any) => x + y
let iter: any = new TestIterator(undefined)
try { iter.reduce(sum) } catch (e: any) { console.log("caught", e.name) }
iter = new TestIterator(null)
try { iter.reduce(sum) } catch (e: any) { console.log("caught", e.name) }
iter = new TestIterator(0)
try { iter.reduce(sum) } catch (e: any) { console.log("caught", e.name) }
iter = new TestIterator("")
try { iter.reduce(sum) } catch (e: any) { console.log("caught", e.name) }
iter = new TestIterator("nonempty")
try { iter.reduce(sum) } catch (e: any) { console.log("caught", e.name) }
iter = new TestIterator(Symbol("s"))
try { iter.reduce(sum) } catch (e: any) { console.log("caught sym", e.name) }
// a REAL object result still drives (single-step, then done)
class Once extends Iterator {
  n = 0
  next() { this.n += 1; return this.n <= 1 ? { value: 5, done: false } : { value: undefined, done: true } }
}
console.log((new Once() as any).reduce(sum, 10))
