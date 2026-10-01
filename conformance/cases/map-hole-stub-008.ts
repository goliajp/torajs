// the only delete in the program sits in a function nothing calls, so
// no hole is ever made; the callback shortens the source with pop and
// the cut indices still arrive as holes
function never(xs: number[]): void {
  delete xs[0]
}
const xs: number[] = [1.5, 2.5, 3.5, 4.5]
const ys = xs.map((v, i) => {
  if (i === 0) {
    xs.pop()
    xs.pop()
  }
  return v * 2
})
console.log(ys.length, 1 in ys, 2 in ys, 3 in ys, JSON.stringify(ys))
