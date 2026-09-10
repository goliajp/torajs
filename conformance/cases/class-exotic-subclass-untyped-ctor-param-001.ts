// A builtin-subclass ctor with an UNTYPED parameter is an implicit
// generic, so its factory is monomorphized per argument type; the
// exotic mint (`super(d)` on Date / Array / Promise) still identifies
// the one class it belongs to (test262 subclass/builtin-objects/
// {Array,Date,Promise}: "constructor calls super with a single
// argument", "super must be called").
class D2 extends Date { constructor(d) { super(d); } }
const d = new D2(0);
console.log(d.getTime(), d instanceof D2, d instanceof Date);
class Sub extends Array { constructor(a) { super(a); } }
const sub = new Sub(42);
console.log(sub.length, Array.isArray(sub), sub instanceof Sub);
class D extends Date { constructor() {} }
try { new D(0); console.log("no throw"); } catch (e) { console.log((e as Error).constructor.name); }
class P extends Promise<any> { constructor(ex) { super(ex); } }
const p = new P((res: any) => res(7));
console.log(p instanceof P, p instanceof Promise);
p.then((v: any) => console.log("resolved", v));
