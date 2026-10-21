// The sibling of promise-then-100k whose receiver is still pending when
// `.then` attaches: `later` yields once (`await null`) before it returns,
// and a binding holds its promise, so the reaction waits in the list until
// later's resumption settles it, then settles the derived promise the loop
// awaits.
function add1(v: number): number { return v + 1 }

async function later(v: number): Promise<number> {
  await null
  return v
}

const n = parseInt(process.argv[2], 10)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const p = later(i)
  total = total + (await p.then(add1))
}
console.log(total)
