// the callback deletes a later index of its typed source: the dense
// loop copy asks per step, skips the hole and leaves one in the product
const xs: number[] = [1.5, 2.5, 3.5, 4.5]
let calls = 0
const ys = xs.map((v, i) => {
  calls = calls + 1
  if (i === 0) delete xs[2]
  return v * 2
})
console.log(calls, ys.length, 2 in ys, 3 in ys, JSON.stringify(ys))
