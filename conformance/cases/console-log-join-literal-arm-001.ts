// A `?:` / `||` whose other arm is a `null` / `undefined` / number /
// boolean literal still answers the binding itself on the path that
// picks it: printing the join must not release the binding, which is
// read again after a few fresh strings were built.
function show(c: boolean, n: number): void {
  const s = 'ab'.repeat(n)
  console.log(c ? s : null)
  console.log(s || undefined)
  console.log(c ? s : 3)
  console.log(c ? s : false)
  console.log(c ? undefined : s)
  const pad = ['x'.repeat(2), 'y'.repeat(2)]
  console.log(s, s.length, pad.join(''))
}
show(true, 2)
show(false, 3)
const t = 'cd'.repeat(2)
console.log(t || undefined)
console.log(t.length > 0 ? t : null)
const pad2 = ['x'.repeat(2), 'y'.repeat(2)]
console.log(t, pad2.join(''))
