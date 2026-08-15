// `instanceof` on the compile-time class lane must walk a DynObj
// receiver's user [[Prototype]] chain against the class's reified
// prototype (405-01 face 2, probes p9/p15): Object.create(C.prototype)
// shapes carry no class_tag, so tag comparison alone answered false
// while the dynamic-target lane (`o instanceof K`) already said true.
class P { m() { return 1 } }
class S extends P { }
class Q { }
const o: any = Object.create((P as any).prototype)
console.log(o instanceof P, o instanceof Q, o instanceof S)
const o2: any = Object.create((S as any).prototype)
console.log(o2 instanceof P, o2 instanceof S)
const K: any = P
console.log(o instanceof K)
const F: any = function (v: number) { (this as any).v = v }
F.prototype = Object.create((P as any).prototype)
const f = new F(3)
console.log(f.v, f instanceof P, f instanceof F)
const plain: any = {}
console.log(plain instanceof P)
