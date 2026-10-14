// closing an iterator helper reads the underlying iterator's `return`
// with [[Get]]: a getter runs, its throw propagates, and an undefined
// answer means there is nothing to call
class Throws extends Iterator<number> {
  next(): any { return { done: false, value: 1 } }
  get return(): any { throw new Error("getter") }
}
for (const mk of [(i: any) => i.map((x: number) => x), (i: any) => i.filter(() => true), (i: any) => i.take(5), (i: any) => i.drop(0), (i: any) => i.flatMap((x: number) => [x])]) {
  const it: any = mk(new Throws())
  it.next()
  try { it.return(); console.log("no throw") } catch (e: any) { console.log(e.constructor.name, e.message) }
}
class Absent extends Iterator<number> {
  next(): any { return { done: false, value: 1 } }
  get return(): any { return undefined }
}
const a: any = new Absent().map((x: number) => x)
a.next()
console.log(JSON.stringify(a.return()))
let closed = 0
class Counted extends Iterator<number> {
  next(): any { return { done: false, value: 1 } }
  get return(): any { return () => { closed++; return {} } }
}
const c: any = new Counted().filter(() => true)
c.next()
console.log(JSON.stringify(c.return()), closed)
console.log([1, 2, 3].values().some((x) => x === 2), new Counted().find((x) => x === 1), closed)
