// A declared field deleted through the any lane or Reflect reads as
// undefined through its typed receiver too; before, the typed read
// loaded the slot's stale value.
type P = { a: number; b: number; c: number }
const o: P = { a: 1, b: 2, c: 3 }
const h: any = o
delete h.c
console.log(o.c, typeof o.c, 'c' in o, Object.keys(o).join(','))
console.log(o.a + o.b, o.c === undefined, String(o.c + 1))
h.c = 5
console.log(o.c, Object.keys(o).join(','), JSON.stringify(o))

const r: P = { a: 1, b: 2, c: 3 }
Reflect.deleteProperty(r, 'c')
console.log(r.c, Object.keys(r).join(','))
r.c = 4
console.log(r.c, Object.keys(r).join(','))

class K {
  a = 1
  b = 'two'
  c = true
}
const k = new K()
delete (k as any).b
delete (k as any).c
console.log(k.a, k.b, k.c, Object.keys(k).join(','), JSON.stringify(k))
console.log(typeof k.b, k.b === undefined, k.c ? 'yes' : 'no')

type S = { a: string; b: string }
const s: S = { a: 'x', b: 'y' }
const t: any = s
delete t.b
console.log(s.b, Object.keys(s).join(','))
t.b = 'z'
console.log(s.b, Object.keys(s).join(','), JSON.stringify(s))

// `c` reads through the any lane on every receiver, `u` too: a delete
// in this program names it. `a` and `b` keep their plain typed reads.
const u: P = { a: 7, b: 8, c: 9 }
console.log(u.a + u.b + u.c)
