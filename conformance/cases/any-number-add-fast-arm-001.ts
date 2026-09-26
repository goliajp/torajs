// `number + any` / `any + number` and ToNumber of an `any` into a
// number slot: the int32 / double encodings the inline arms decode and
// every value that must still reach the runtime kernels.

const f64 = new Float64Array(3)
const u32 = new Uint32Array(f64.buffer)
u32[1] = 0xfff70000
u32[3] = 0xfff90001
u32[5] = 0xffff1234
const vals: any[] = [
  7, -3, 2147483647, -2147483648, 2.5, -0, 0, NaN, Infinity, -Infinity, 1e300,
  f64[0], f64[1], f64[2],
  'x', '', '12', null, undefined, true, false,
  { valueOf: () => 40 }, [5], [1, 2],
]
const base = [0.5, -0, 1e308, NaN]
const out: string[] = []
for (const b of base) {
  for (const v of vals) {
    const l = b + v
    const r = v + b
    out.push(String(l) + '/' + String(r) + '/' + typeof l + (Object.is(l, -0) ? '-0' : ''))
  }
}
console.log(out.join(' '))

let total = 0.25
for (let i = 0; i < 200000; i = i + 1) {
  const v: any = vals[i % 11]
  total = total + v
}
console.log(total)

for (const bad of [Symbol('s'), 10n] as any[]) {
  try {
    console.log(1.5 + bad)
  } catch (e) {
    console.log(e instanceof TypeError)
  }
}

function num(x: number): number {
  return x * 2
}
const nums: number[] = []
for (const v of vals) {
  const n: number = +v
  nums.push(num(v), n)
}
console.log(nums.map((n) => (Object.is(n, -0) ? '-0' : String(n))).join(','))
