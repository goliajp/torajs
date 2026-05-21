// P10.3-A1 narrow MVP — `for await (decl of iter)` on Array<Promise<T>>.
// Parser strips the optional `await` after `for`, sets is_async on the
// for-of head; try_parse_for_of wraps elem_expr in Member.value (the
// await desugar). ssa_lower's ForOf lowering accepts the wrapper,
// resolves src via the inner Index, and the body's elem load goes
// through promise_get_value (P10.3 prereq d2a7c61 made that route
// work on Index obj). AsyncIterator protocol + AsyncGenerator + for-
// await over user-iterables deferred to later sub-A's.

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
