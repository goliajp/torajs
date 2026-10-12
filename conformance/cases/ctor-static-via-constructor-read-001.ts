// a builtin constructor read off a value through `constructor` keeps
// its statics callable (no `Object` ident in the program)
function m() {
  const o: any = {}
  const O: any = o.constructor
  return O.keys({ a: 1 })
}
console.log(m())

const o: any = { b: 2 }
console.log(o.constructor.values(o))
const p = { c: 3 }
console.log(p.constructor.entries(p))

function g(C: any) {
  return C.keys({ d: 4, e: 5 })
}
console.log(g(o.constructor))

const { constructor: D } = o
console.log(D.keys(o), Reflect.get(o, "constructor").entries({ f: 6 }))
