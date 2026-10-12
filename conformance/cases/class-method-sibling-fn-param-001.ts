// a fn-typed param of a method name two unrelated classes declare: the
// member call stays unresolved until lowering, and the arrow reached
// the bare-pointer param as a closure cell (SIGBUS)
class K { m(f: () => number) { return f() } }
class Base { m(f: () => number) { return f() } }
class Sub extends Base { m(f: () => number) { return f() * 10 } }
console.log(new K().m(() => 1), new Sub().m(() => 2))
