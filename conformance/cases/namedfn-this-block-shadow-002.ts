// A block that rebinds a promoted declaration's name owns only the
// reads INSIDE it. Rotation 595 refused the whole name, which is safe
// but sends every other value use of that declaration back to the
// answer it gave before the promote pass existed — one block anywhere
// reusing the spelling was enough.

function t(a: any) {
  return "a=" + a + "/" + typeof (this as any)
}

// the block's own reads answer the block binding
{
  const t = 3
  console.log(t)
}
if (true) {
  const t = 4
  console.log(t)
}
for (let i = 0; i < 1; i++) {
  const t = 5
  console.log(t)
}
{
  {
    const t = 6
    console.log(t)
  }
}
{
  const t = 7
  console.log(`${t}`)
}

// value uses OUTSIDE every such block keep the forwarder. The two
// receiver-bearing arms (a property store, an object-literal field)
// are refused by a THIRD copy of the whole-name rule, in the
// namedfn_recv_cb collectors, and are not part of this case.
const alias: any = t
console.log(alias(1))

function take(f: any) {
  return f(4)
}
console.log(take(t))

// a value use written BEFORE the shadowing block is the same site kind
const early: any = t
{
  const t = 8
  console.log(t)
}
console.log(early(5))
