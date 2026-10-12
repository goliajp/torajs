// a fn-typed param of a method name several unrelated classes declare
class K { m(f: () => number) { return f() } }
class Base { m(f: () => number) { return f() } }
class Sub extends Base { m(f: () => number) { return f() * 10 } }
class Deep extends Sub { m(f: () => number) { return f() * 100 } }
class Third { m(f: () => number) { return f() + 1000 } }
function seven(): number { return 7 }
const n = 5
console.log(new K().m(() => 1), new Sub().m(() => 2))
console.log(new Third().m(() => 3), new Deep().m(() => 4))
console.log(new K().m(seven), new Third().m(seven), new Base().m(seven))
console.log(new K().m(() => n), new Third().m(function () { return n + 1 }))
const subs: Base[] = [new Base(), new Sub(), new Deep()]
for (const b of subs) console.log(b.m(() => n * 2), b.m(seven))
const k = new K()
let acc = 0
for (let i = 0; i < 3; i++) acc += k.m(() => i)
console.log(acc)
