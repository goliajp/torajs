// A derived class in the runtime-value lane with field initializers and
// NO constructor: the implicit `constructor(...args) { super(...args) }`
// still runs, and the initializers run after it returns.
class R { v = 1; tag: string; constructor(t: string = "t") { this.tag = t } hi() { return "r" } }
class Plain { v = 1 }
function mk(Base: any): any { return class extends Base { w = 2; both = (this as any).v + this.w } }
function mkArgs(Base: any, args: number[]): any { return class extends Base { n = args.length } }
const a: any = new (mk(R))("x"), b: any = new (mk(R))(), c: any = new (mk(Plain))("ignored");
console.log(a.v, a.tag, a.w, a.both, "|", b.tag, b.both, "|", c.v, c.w, c.both, Object.keys(c).join());
console.log(a.hi(), a instanceof R, c instanceof Plain, new (mk(mk(Plain)))().both);
console.log(new (mkArgs(Plain, [1, 2, 3]))(9, 9).n, new (mkArgs(R, []))("q").tag);
