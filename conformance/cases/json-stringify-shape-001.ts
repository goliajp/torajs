// S1-A2 attack A1 — descriptor-driven one-call JSON.stringify for
// primitive-only static layouts (jsb_stringify_shape). Covers the
// bench shape, escape/UTF-16 values, digit edges, and field counts.
function makeRecord(i: number) {
  return { id: i, name: 'row', score: i * 7, active: (i & 1) === 0 }
}
let total = 0
for (let i = 0; i < 3; i = i + 1) {
  const s = JSON.stringify(makeRecord(i))
  total = total + s.length
  console.log(s)
}
console.log(total)
console.log(JSON.stringify({ a: -5, b: 'x"y\\z', c: true }))
console.log(JSON.stringify({ t: '中文値', n: 9007199254740991 }))
console.log(JSON.stringify({ nl: 'a\nb\tc', z: 0 }))
console.log(JSON.stringify({ one: 1 }))
console.log(
  JSON.stringify({ a1: 1, a2: 2, a3: 3, a4: 4, a5: 5, a6: 6, a7: 7, a8: 'v' })
)
