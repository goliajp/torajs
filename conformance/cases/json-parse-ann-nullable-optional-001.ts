// An annotated JSON.parse whose value has `T | null` fields holding
// null, optional fields left out, keys in another order and extra
// keys: each slot keeps exactly what the text says.
interface A { a: string | null; n: number }
const x: A = JSON.parse('{"n":1,"a":null,"extra":true}')
console.log(x.a === null, x.n)
const x2: A = JSON.parse('{"a":"s","n":2}')
console.log(x2.a, x2.n)
type B = { a?: string; n: number; k: number | null }
const y: B = JSON.parse('{"n":2,"k":null}')
console.log(y.a === undefined, y.n, y.k === null)
const y2: B = JSON.parse('{"k":3,"a":"t","n":4}')
console.log(y2.a, y2.n, y2.k)
const z: { a: string | null; xs: number[] }[] = JSON.parse('[{"a":null,"xs":[1,2]},{"xs":[],"a":"q"}]')
console.log(z.length, z[0].a === null, z[0].xs.length, z[1].a, z[1].xs.length)
class C { name: string | null = null; v = 0 }
const c: C = JSON.parse('{"v":5,"name":null}')
console.log(c.name === null, c.v)
