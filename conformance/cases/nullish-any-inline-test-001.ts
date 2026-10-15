// `lhs ?? rhs` with an `any` lhs decides on the value's own encoding:
// only null and undefined (an array hole reads as undefined) take the
// right-hand side, and every other value — 0, -0, NaN, '', false,
// short and long strings, objects, symbols, bigints — is the result
// as it is, with the right-hand side never evaluated.

const f64 = new Float64Array(2)
const u32 = new Uint32Array(f64.buffer)
u32[1] = 0xfff80000
u32[3] = 0x7ff00001
const holey: any[] = [1, , 3]
const vals: any[] = [
  null, undefined, holey[1], 0, -0, NaN, Infinity, -1, 2147483647, 1.5,
  '', 'a', 'abcde', 'abcdef', 'a much longer string value', false, true,
  {}, [], [0], Symbol.iterator, 0n, 12n, f64[0], f64[1], () => 1,
]
let calls = 0
function rhs(): any {
  calls = calls + 1
  return 'R'
}
const out: string[] = []
for (const v of vals) {
  const r = v ?? rhs()
  out.push(typeof r + (Object.is(r, -0) ? '-0' : '') + (r === v ? '=' : '!'))
}
console.log(out.join(' '), calls)

const d: Record<string, number> = {}
const alias: any = d
alias.s = 'str'
alias.n = null
alias.u = undefined
alias.z = 0
for (const k of ['s', 'n', 'u', 'z', 'missing']) {
  console.log(k, d[k] ?? -1)
}

function pick(i: number): any {
  return vals[i]
}
let hits = 0
for (let i = 0; i < 260000; i = i + 1) {
  if ((pick(i % vals.length) ?? null) === null) hits = hits + 1
}
console.log(hits)
