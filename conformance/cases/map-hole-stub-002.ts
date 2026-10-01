// a delete punches a hole into a typed source: the product keeps the
// hole, and the callback never sees the index
const xs: number[] = [1, 2, 3, 4]
delete xs[1]
let calls = 0
const ys = xs.map((v) => { calls = calls + 1; return v * 10 })
console.log(calls, ys.length, 1 in ys, 0 in ys, JSON.stringify(ys))
const ss: string[] = ['a', 'b', 'c']
delete ss[2]
const ts = ss.map((s) => s + s)
console.log(ts.length, 2 in ts, JSON.stringify(ts))
