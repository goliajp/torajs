// An array pattern over a string reads the string's code points
// (§22.1.5.1), each one a string: a surrogate pair is one element, a
// default fills a slot past the end, a rest is an array of the rest,
// and a nullish string source throws a TypeError.
function pair(s: string): string {
  const [a, b] = s
  const [c, ...rest] = s
  return a + '|' + b + '|' + c + '|' + rest.join(',') + '|' + rest.length
}
console.log(pair('x😀yz'))
console.log(pair('😀x'))
console.log(pair('q'))
const [e1, e2 = 'dflt', e3] = 'k'
console.log(e1, e2, e3, typeof e3)
const [, sk, ...tl] = 'a😀bc'
console.log(sk, sk.length, tl, tl.length)
const [g1, g2] = '😀x'
console.log(g1, g1.length, g2, g1.codePointAt(0))
for (const [h1, h2] of ['ab', 'c😀']) {
  console.log(h1, h2, h1.toUpperCase())
}
function param([p1, p2]: string): string {
  return p2 + p1
}
console.log(param('mn'), param('😀z'))
function* gen(s: string) {
  const [y1, y2] = s
  yield y1
  yield y2
}
console.log([...gen('uv')].join('+'))
const words = ['one', 'two']
const [w0, w1] = words[1]
console.log(w0, w1)
try {
  const [z0] = words[5]
  console.log('no throw', z0)
} catch (e) {
  console.log(e instanceof TypeError)
}
const view = 'abcdef'.split('c')[1]
const [v0, v1] = view
console.log(v0, v1, view)
let tot = 0
for (let i = 0; i < 100000; i = i + 1) {
  const [w, ...ws] = 'r'.repeat(3) + i
  tot = tot + w.length + ws.length
}
console.log(tot)
const pad = ['x'.repeat(2), 'y'.repeat(2)]
console.log(e1, sk, g1, g2, pad.join(''))
