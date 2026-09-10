// `super(...xs)` into a parent constructor WITH declared parameters:
// the ctor argv lane now unpacks the declared slots off the
// materialized list (past the end answers undefined) and converts
// each to the parameter's own type the way a positional argument
// is, next to the true argc / argv pair. Before, such a parent was
// off the lane and the site died loud at the spread callee boxing.
class P { constructor(a: number, b: string) { console.log("P", a, b, arguments.length, arguments[2]); } }
const xs: any[] = [1, "x", 3];
class C extends P { constructor() { super(...xs); } }
new C();
class D extends P { constructor() { super(7, ..."yz"); } }
new D();
function* g(): Generator<any> { yield 4; yield "w"; yield 5; yield 6; }
class E extends P { constructor() { super(...g()); } }
new E();
const short: any[] = [9];
class F extends P { constructor() { super(...short); console.log("F"); } }
new F();
class Q { constructor(a: any, b = 2) { console.log("Q", a, b); } }
class R extends Q { constructor() { super(...[1]); } }
new R();
class S { constructor(public n: number) { console.log("S", n * 2); } }
class T extends S { constructor() { super(...[21]); } }
new T();
