// rc retain-at-return — returning a borrowed binding (param / for-of
// binding / member alias) must hand the caller a +1 owned reference.
// Regression: `return s` (s: string param) forwarded the caller's own
// +0 reference; the caller's scope-end drop then freed the heap block
// while the returned alias was still live, and the next allocation
// recycled the pool block — the alias printed foreign bytes.

// param passthrough — arg dies at the inner block close, returned
// alias must survive
function id(s: string): string {
  return s
}
let n = 10
let t: string = ''
{
  let s = 'abcdefgh'.repeat(n)
  t = id(s)
}
let junk = 'zzzzzzzz'.repeat(n)
console.log(junk.length)
console.log(t)

// for-of binding passthrough — the array dies before the returned
// element alias is read
function firstLong(xs: string[]): string {
  for (const x of xs) {
    if (x.length > 4) {
      return x
    }
  }
  return ''
}
let u: string = ''
{
  let xs = ['ab', 'qrstuvwx'.repeat(n), 'cd']
  u = firstLong(xs)
}
let junk2 = 'yyyyyyyy'.repeat(n)
console.log(junk2.length)
console.log(u)

// static literal passthrough — retain must stay a no-op on .rodata
console.log(id('hello'))

// owned-local return still balances (no retain): multi-return of the
// same owned local must not leak or double-free
// (body uses a literal repeat count: referencing a top-level `let`
// from a fn body trips a pre-existing unrelated compile abort)
function multiRet(flag: boolean): string {
  let s = 'ijklmnop'.repeat(10)
  if (flag) {
    return s
  }
  return s
}
console.log(multiRet(true).length)
console.log(multiRet(false).length)
