// `.length` on `any` receivers: the Str cells the inline arm answers
// and every encoding that must still reach the runtime helper.

const long = 'x'.repeat(40) + 'y'
const wide = 'héllo wörld, ünïcode ' + 'テキスト' + '😀'
const vals: any[] = [
  long,
  'static-literal-longer-than-short',
  'ab',
  '',
  wide,
  long.slice(3, 30),
  wide.substring(2, 9),
  long.slice(1),
  [1, 2, 3],
  new String('wrapped'),
  { length: 42 },
  new Uint8Array(5),
  new Proxy({}, { get: (_t: any, k: any) => (k === 'length' ? 'proxied' : undefined) }),
  7,
  true,
]
const out: any[] = []
for (const v of vals) out.push(v.length)
console.log(out.join(','))

let sum = 0
for (let i = 0; i < 300000; i = i + 1) {
  const s: any = vals[i % 8]
  sum = sum + s.length
}
console.log(sum)

for (const bad of [null, undefined] as any[]) {
  try {
    console.log(bad.length)
  } catch (e) {
    console.log(e instanceof TypeError)
  }
}
