// Deletes that the first fixture leaves out: a bracketed string key, a
// delete straight through a typed receiver whose member is optional,
// and the own-property tests (`in`, hasOwnProperty, Object.hasOwn) read
// after the delete and after the field comes back.
type Q = { a: number; c?: number; s?: string }
const q: Q = { a: 1, c: 2, s: 'x' }
delete q.c
console.log(q.c, 'c' in q, q.hasOwnProperty('c'), Object.hasOwn(q, 'c'))
console.log(Object.keys(q).join(','), JSON.stringify(q))
delete q['s']
console.log(q.s, typeof q.s, 's' in q, Object.hasOwn(q, 's'))
q.c = 9
console.log(q.c, 'c' in q, q.hasOwnProperty('c'), Object.hasOwn(q, 'c'))

type P = { a: number; b: number; k: string }
const p: P = { a: 1, b: 2, k: 'kk' }
const h: any = p
delete h['k']
console.log(p.k, p.k === undefined, 'k' in p, Object.hasOwn(p, 'k'))
console.log(p.hasOwnProperty('k'), p.hasOwnProperty('a'), Object.keys(p).join(','))

class M {
  x: number = 4
  y?: number = 5
}
const m = new M()
delete m.y
console.log(m.x, m.y, 'y' in m, m.hasOwnProperty('y'), Object.hasOwn(m, 'y'))
console.log(JSON.stringify(m))
m.y = 6
console.log(m.y, 'y' in m, JSON.stringify(m))
