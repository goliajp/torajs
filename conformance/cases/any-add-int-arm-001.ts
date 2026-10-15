// `any + n` and `n + any` where `n` is an integer (a literal, an
// integer counter): an int32 encoding adds as an integer and stays an
// int32 while the sum fits; a sum past either int32 edge, a double
// encoding (-0, NaN, the infinities, a fraction) and every value that
// is not a number answer what §13.15.3 ApplyStringOrNumericBinaryOperator
// answers.

const f64 = new Float64Array(3)
const u32 = new Uint32Array(f64.buffer)
u32[1] = 0xfff70000
u32[3] = 0xfff90001
u32[5] = 0xffff1234
const vals: any[] = [
  7, -3, 0, 2147483647, -2147483648, 2147483646, -2147483647, 2.5, -2.5, -0, NaN,
  Infinity, -Infinity, 1e300, 9007199254740991, -9007199254740991, 4294967296,
  f64[0], f64[1], f64[2],
  'x', '', '12', null, undefined, true, false,
  { valueOf: () => 40 }, [5], [1, 2], { toString: () => 'o' },
]
const out: string[] = []
function show(v: any): string {
  return String(v) + ':' + typeof v + (Object.is(v, -0) ? '-0' : '')
}
for (const v of vals) {
  out.push(show(v + 1) + ' ' + show(1 + v) + ' ' + show(v + 0) + ' ' + show(0 + v))
  out.push(show(v + -1) + ' ' + show(v + 2147483647) + ' ' + show(-2147483648 + v))
}
console.log(out.join('\n'))

let k = 0
const ints: string[] = []
for (let i = -3; i < 4; i = i + 1) {
  for (const v of vals) {
    k = k + 1
    const s: any = v + i
    const t: any = i + v
    if (k % 7 === 0) ints.push(show(s) + '|' + show(t))
  }
}
console.log(ints.join(' '))

const d: Record<string, number> = {}
const keys = ['a', 'b', 'c']
for (let i = 0; i < 3000; i = i + 1) {
  const key = keys[i % 3]
  d[key] = (d[key] ?? 2147483640) + 1
}
console.log(d.a, d.b, d.c, typeof d.a)

let acc: any = 2147483600
for (let i = 0; i < 100; i = i + 1) acc = acc + 1
console.log(acc, typeof acc)
acc = -2147483600
for (let i = 0; i < 100; i = i + 1) acc = acc + -1
console.log(acc)

for (const bad of [Symbol('s'), 10n] as any[]) {
  try {
    console.log(bad + 1)
  } catch (e) {
    console.log(e instanceof TypeError)
  }
}
