// the same shape with the cut made by a length write
function never(xs: number[]): void {
  delete xs[0]
}
const xs: number[] = [1.5, 2.5, 3.5, 4.5]
const ys = xs.map((v, i) => {
  if (i === 0) xs.length = 2
  return v * 2
})
console.log(ys.length, 1 in ys, 2 in ys, 3 in ys, JSON.stringify(ys))
