// A callback method hands the receiver itself to the callback as O
// (§23.1.3.15 forEach step 7.c.ii and its siblings), so a width change
// through O is a change to the receiver: a fraction written, or an
// element deleted, through o reaches xs.
const xs = [1, 2, 3]
xs.forEach((v, i, o) => { if (i === 0) o[1] = 0.5 })
console.log(xs.join(","))
function viaParam(ys: number[]) {
  const out: number[] = []
  ys.forEach((v, i, o) => {
    if (i === 0) delete o[2]
    out.push(v)
  })
  return out.join(",") + " " + (2 in ys)
}
console.log(viaParam([7, 8, 9, 10]))
const zs = [4, 5, 6]
const tot = zs.reduce((acc, v, i, o) => { if (i === 0) o[2] = 1.5; return acc + v }, 0)
console.log(tot, zs.join(","))
const ws = [1, 2, 3, 4]
console.log(ws.filter((v, i, o) => { if (i === 0) delete o[1]; return true }).join("|"))
