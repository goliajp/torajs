// `any + any` over doubles: the boxed sum keeps -0, the infinities and
// NaN, and a NaN read back out of a Float64Array is still a NaN.
const xs: any[] = [-0, 0.5, -0.5, 1e308, -1e308, Infinity, -Infinity, NaN, 2147483647, 0.25]
const f = new Float64Array(1)
const u = new Uint32Array(f.buffer)
u[1] = 0xfff80001
const odd: any = f[0]
xs.push(odd)
for (const a of xs) {
  let line = ''
  for (const b of xs) {
    const s = a + b
    line += (Object.is(s, -0) ? '-0' : String(s)) + ' '
  }
  console.log(line)
}
let acc: any = -0
for (let i = 0; i < 3; i = i + 1) acc = acc + xs[i % 1]
console.log(Object.is(acc, -0), acc + odd !== acc + odd)
