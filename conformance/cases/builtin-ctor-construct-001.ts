// [[Construct]] through a builtin-constructor VALUE — `new N(5)`
// through a `const N: any = Number` binding, `new globalThis.Array(3)`
// (the roadmap first-class-ctor follow-up). Wrapper families mint
// real wrapper cells (construct differs from call: the answer is an
// object); Object / Array share the call-form semantics; Date covers
// the zero-arg (now) and one-number (ms) mints; Map / Set mint fresh
// empties. Families beyond these keep the loud catchable TypeError.
const N: any = Number;
const nw = new N(5);
console.log(typeof nw, nw.valueOf(), nw instanceof Number);
const S: any = String;
const sw = new S(42);
console.log(typeof sw, sw.valueOf(), sw.length);
const B: any = Boolean;
console.log(typeof new B(1), new B(0).valueOf());
const A: any = Array;
console.log(new A().length, new A(3).length);
const D: any = Date;
console.log(new D(0).getTime());
const M: any = Map;
const m = new M();
m.set("k", 1);
console.log(m.get("k"), m.size);
console.log(new (globalThis as any).Array(4).length);
const bound: any = A.bind(null);
console.log(new bound(2).length);
console.log("done");
