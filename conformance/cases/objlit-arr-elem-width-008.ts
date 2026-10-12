// a yielded literal and a same-shape fractional literal elsewhere, in
// both registration orders
function* g() { yield { x: 1, y: 2 } }
const z = { x: 0.5, y: 1 }
for (const p of g()) console.log(p.x, p.y, z.x)

const w = { a: 2.5, b: 1 }
function* h() { yield { a: 3, b: 4 }; yield { a: 5, b: 6 } }
for (const p of h()) console.log(p.a + p.b, w.a)
