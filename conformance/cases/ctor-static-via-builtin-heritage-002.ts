// builtin statics reached through a subclass's heritage link
class A extends Array {}
const P: any = Object.getPrototypeOf(A)
console.log(P.of(1, 2))
console.log((A as any).from([1]))

class CP extends Promise<number> {}
const q: any = (CP as any).__proto__
q.resolve(1).then((v: number) => console.log(v))
