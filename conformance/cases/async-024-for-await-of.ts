// P10.3-A1 — `for await (decl of iter)` on Array<Promise<T>>.
// Parser strips the optional `await` after `for` and sets is_await on
// Stmt::ForOf; the element stays a plain `src[i]` Index (hole Z
// removed the `.value` Member wrap). ssa_lower's array lane reads the
// checker's Promise verdict on the element and routes the load
// through promise_get_value; non-thenable elements await to
// themselves per §27.2.

// Path A — number array
let pn: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)]
for await (const v of pn) {
  console.log('num', v)
}

// Path B — string array
let ps: Promise<string>[] = [Promise.resolve('a'), Promise.resolve('b')]
for await (const s of ps) {
  console.log('str', s)
}

// Path C — boolean array
let pb: Promise<boolean>[] = [Promise.resolve(true), Promise.resolve(false)]
for await (const b of pb) {
  console.log('bool', b)
}

// Path D — empty array (zero iterations)
let empty: Promise<number>[] = []
for await (const x of empty) {
  console.log('UNREACHABLE')
}
console.log('after-empty')
