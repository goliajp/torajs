// a boxed source shortened mid-walk takes the dense arm's length exit
// into the hole arm; one with a hole punched in takes the exotic arm
const xs: any[] = [1, 'b', 3, 4]
let calls = 0
const ys = xs.map((v: any, i: number) => {
  calls = calls + 1
  if (i === 1) xs.length = 2
  return v
})
console.log(calls, ys.length, 1 in ys, 2 in ys, JSON.stringify(ys))
const zs: any[] = [1, 2, 3, 4]
const ws = zs.map((v: any, i: number) => {
  if (i === 0) delete zs[2]
  return v * 2
})
console.log(ws.length, 2 in ws, 3 in ws, JSON.stringify(ws))
