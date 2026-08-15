class TestIterator extends Iterator {
  constructor(value: any) { super(); this.value = value }
  next() { return this.value }
}
const sum = (x: any, y: any) => x + y
for (const v of [undefined, null, 0, false, ""]) {
  const iter: any = new TestIterator(v)
  try { iter.reduce(sum) } catch (e: any) { console.log("caught", e.name) }
}
const iter2: any = new TestIterator(Symbol(""))
try { iter2.reduce(sum) } catch (e: any) { console.log("caught sym", e.name) }
console.log("after")
