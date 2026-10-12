// a class extending a builtin has the builtin constructor as its
// [[Prototype]]; reading it back reaches the builtin's statics with
// no `constructor` key in the program
class A extends Object {}
class B extends A {}
const P: any = Object.getPrototypeOf(A)
console.log(P.keys({ a: 1 }))
const R: any = Reflect.getPrototypeOf(A)
console.log(R.entries({ c: 3 }))
const Q: any = (A as any).__proto__
console.log(Q.values({ b: 2 }))
const G: any = Object.getPrototypeOf(Object.getPrototypeOf(B))
console.log(G.keys({ d: 4 }))
console.log((A as any).values({ g: 7 }), (B as any).keys({ h: 8 }))

class L extends Array {}
const LP: any = Object.getPrototypeOf(L)
console.log(LP.hasOwnProperty("of"), LP.name, LP.length)

class CP extends Promise<number> {}
const q: any = (CP as any).__proto__
console.log(q.hasOwnProperty("resolve"), q.name)
