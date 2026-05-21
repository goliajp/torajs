// P10.3 prerequisite — `await <expr>` where expr is Index or Member
// access yielding Promise<T>. Pre-fix tora panicked "member access on
// non-object Promise (.value)" because the ssa_lower whitelist for
// the await→.value desugar covered Ident- and Call-shaped obj only.
// Extending the whitelist via expr_types lookup picks up any expr
// whose check-time Type is Promise<T>, including Index / Member /
// any future Promise-producing shape.

// Path A — await Array<Promise<T>>[i] inline
let ps: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)]
for (let i = 0; i < ps.length; i++) {
  let v = await ps[i]
  console.log('idx', v)
}

// Path B — await on struct field Promise<T>
type Box = { p: Promise<string> }
let b: Box = { p: Promise.resolve('boxed') }
let s = await b.p
console.log('member', s)

// Path C — await on nested Index of Array<Array<Promise<T>>>
let grid: Promise<number>[][] = [
  [Promise.resolve(10), Promise.resolve(11)],
  [Promise.resolve(20), Promise.resolve(21)],
]
let g00 = await grid[0][0]
let g11 = await grid[1][1]
console.log('grid', g00, g11)
