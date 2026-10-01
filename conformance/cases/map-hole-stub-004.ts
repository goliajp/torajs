// a callback that shortens its typed source: the indices past the new
// length are absent (no header bit says so) and arrive as holes
const xs: number[] = [1, 2, 3, 4, 5]
let calls = 0
const ys = xs.map((v, i) => {
  calls = calls + 1
  if (i === 0) xs.length = 2
  return v * 2
})
console.log(calls, ys.length, 1 in ys, 2 in ys, 4 in ys, JSON.stringify(ys))
const zs: number[] = [10, 20, 30]
const ws = zs.map((v) => { zs.pop(); return v + 1 })
console.log(ws.length, JSON.stringify(ws), 1 in ws, 2 in ws)
