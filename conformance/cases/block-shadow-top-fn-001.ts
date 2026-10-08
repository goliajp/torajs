// a block binding that shadows a top-level function is the one its
// block reads: as a callback, captured by an arrow, and through a copy
function sq(x: number): number { return x * x }
function main() {
  {
    let sq = (x: number) => x + 1000
    console.log([1, 2].reduce((a, b) => a + sq(b), 0))
    const h = (b: number) => sq(b)
    console.log(h(1))
  }
}
main()
{
  let sq = (x: number) => x + 1000
  const h = (b: number) => sq(b)
  console.log(h(1))
  const k = (b: number) => { const f = sq; return f(b) }
  console.log(k(1))
  console.log([1, 2].map(sq).join(","))
  console.log([1, 2].map((v) => sq(v)).join(","))
}
