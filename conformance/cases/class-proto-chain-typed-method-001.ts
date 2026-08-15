// A dynobj receiver reaching a real class's TYPED method through the
// user [[Prototype]] chain must ride the __cmany_ twin, not the mono
// body's baked struct offsets (405-01 face 2, probes p13/p14): the
// chain arm's receiver is always a dynobj, never the owning class's
// struct layout, so the mono path read nanbox bits as field values.
class P {
  x: number
  constructor(x: number) { this.x = x }
  m() { return this.x + 1 }
  anyRead() { return (this as any).w }
  free() { return 7 }
}
const o: any = Object.create((P as any).prototype)
o.x = 30
console.log(o.m())
console.log((P as any).prototype.m.call(o))
const F: any = function (v: number) { (this as any).x = v * 10 }
F.prototype = Object.create((P as any).prototype)
const f = new F(3)
console.log(f.x, f.m())
o.w = 5
console.log(o.anyRead(), o.free())
