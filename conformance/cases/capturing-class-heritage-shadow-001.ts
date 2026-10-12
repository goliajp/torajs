// A heritage names the binding the nearest scope gives it: a parameter
// that shadows a top-level class is a value, and super() constructs
// whatever it holds — a class, a function, or a Proxy over one.
class base {
  tag: string;
  constructor(nt: any) {
    console.log("class", new.target === nt);
    this.tag = "c";
  }
}
function baseFunc(this: any, nt: any) {
  console.log("func", new.target === nt);
  this.tag = "f";
}
function testBase(base: any) {
  class inst extends base {
    constructor(nt: any) {
      super(nt);
    }
  }
  const o: any = new inst(inst);
  console.log(o.tag, Object.getPrototypeOf(o) === inst.prototype);
  class dflt extends base {}
  const d: any = new dflt(dflt);
  console.log(d.tag, Object.getPrototypeOf(d) === dflt.prototype);
}
testBase(base);
testBase(baseFunc);
const handler: any = {};
const p = new Proxy(baseFunc, handler);
testBase(p);
handler.construct = (target: any, args: any, nt: any) => Reflect.construct(target, args, nt);
testBase(p);
