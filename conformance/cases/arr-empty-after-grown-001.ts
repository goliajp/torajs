// an empty array made right after a grown one was released starts
// empty and behaves as any fresh array, whatever its storage held

class Tok {
  kind: number
  constructor(kind: number) {
    this.kind = kind
  }
}

function build(n: number): Tok[] {
  const out: Tok[] = []
  for (let i = 0; i < n; i = i + 1) out.push(new Tok(i))
  return out
}

let sum = 0
for (let r = 0; r < 5; r = r + 1) {
  const a = build(20 + r)
  sum = sum + a.length + a[a.length - 1].kind
}
console.log(sum)

function grownThenEmpty(): void {
  const g: number[] = []
  for (let i = 0; i < 40; i = i + 1) g.push(i * 2)
  console.log(g.length, g[39])
}
grownThenEmpty()

const e: number[] = []
console.log(e.length, e[0], e[5], JSON.stringify(e), e.indexOf(78), e.includes(0))
e.push(7)
console.log(e.length, e[0], e[1], e.join(','))

grownThenEmpty()
const holes: number[] = []
holes.length = 3
console.log(holes.length, 0 in holes, 1 in holes, JSON.stringify(holes))
holes[5] = 1
console.log(holes.length, 2 in holes, 5 in holes, holes[4], holes[5])

grownThenEmpty()
const q: string[] = []
q.unshift('b')
q.unshift('a')
q.push('c')
console.log(q.shift(), q.length, q.join('|'))
q.reverse()
q.sort()
console.log(q.join('|'), q.slice(0, 1).join('|'), q.concat(['d']).join('|'))

grownThenEmpty()
const objs: Tok[] = []
console.log(objs.map((t) => t.kind).length, objs.filter((t) => t.kind > 0).length)
for (let i = 0; i < 300; i = i + 1) objs.push(new Tok(i))
let ks = 0
for (const t of objs) ks = ks + t.kind
console.log(objs.length, ks)

grownThenEmpty()
const anyArr: any[] = []
anyArr.push(1, 'two', null, undefined, { x: 3 })
console.log(anyArr.length, JSON.stringify(anyArr))
