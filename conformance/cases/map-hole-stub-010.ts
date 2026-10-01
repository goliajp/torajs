// an accessor index on the source: map reads it through the getter and
// the product holds the value, not a hole
const xs: any[] = [1, 2, 3]
Object.defineProperty(xs, 1, {
  get: function () {
    return 20
  },
  configurable: true,
})
const ys = xs.map((v: any) => v + 1)
console.log(ys.length, 1 in ys, JSON.stringify(ys))
const zs: number[] = [1.5, 2.5, 3.5]
Object.defineProperty(zs, 0, {
  get: function () {
    return 9.5
  },
  configurable: true,
})
const ws = zs.map((v) => v * 2)
console.log(ws.length, 0 in ws, JSON.stringify(ws))
