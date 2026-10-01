// a callback that may punch a hole and also shortens the source: the
// indices past the new length are absent without any hole being made
const xs: number[] = [1.5, 2.5, 3.5, 4.5, 5.5]
const ys = xs.map((v, i) => {
  if (i === 0) {
    xs.pop()
    xs.pop()
  }
  if (v > 100) delete xs[0]
  return v * 2
})
console.log(ys.length, 2 in ys, 3 in ys, 4 in ys, JSON.stringify(ys))
