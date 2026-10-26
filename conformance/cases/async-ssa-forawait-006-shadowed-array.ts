const values = Array.prototype.values
;(Array.prototype as any)[Symbol.iterator] = function (this: number[]) {
  let i = 0
  const a = this
  return { next: () => (i < a.length ? { value: a[i++] + 0.5, done: false } : { value: undefined, done: true }) }
}
async function f() {
  const out: number[] = []
  for await (const v of [1, 2, 3]) out.push(v)
  console.log(out.join(","))
}
f()
console.log(typeof values)
