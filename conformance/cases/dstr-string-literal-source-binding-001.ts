// An array pattern over a string literal steps the string's code
// points into an array; the bindings read that array's elements and
// must not release them when their scope ends — the array releases
// them once more, and a string built afterwards would land in their
// storage.
function pair(): string {
  const [a, b] = 'x😀'
  const [c, ...rest] = 'yzw'
  return a + b + c + rest.join('')
}
console.log(pair())
console.log(pair(), 'k'.repeat(3))
{
  const [p, q] = 'st'
  console.log(p, q, p.length)
}
const after = ['u'.repeat(2), 'v'.repeat(2)]
console.log(after.join('-'))
for (let i = 0; i < 3; i = i + 1) {
  const [h, ...t] = 'mno'
  console.log(i, h, t.length, t[0])
}
