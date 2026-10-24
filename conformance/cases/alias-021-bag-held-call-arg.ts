// A value that lives in an expando bag reaches typed code only through
// a read of `any`, which owns what it answers. Releasing the bag while
// the callee still reads its parameter must leave the value alive: the
// struct cell's bag, a Date's, a RegExp's and an array's.

class P { v = 7 }
class H { p: P = new P() }
class Q { n = 1 }

let q: Q | null = new Q()
;(q as any).xs = [new P()]
function useQ(p: P): number { q = null; const r = new P(); r.v = 1; return p.v + r.v * 0 }
console.log(useQ(((q as any).xs as P[])[0]))

let q2: Q | null = new Q()
;(q2 as any).h = new H()
function getH(): H { return (q2 as any).h as H }
function useH(p: P): number { q2 = null; const r = new P(); r.v = 2; return p.v + r.v * 0 }
console.log(useH(getH().p))

let d: Date | null = new Date(0)
;(d as any).h = new H()
function useD(p: P): number { d = null; const r = new P(); r.v = 3; return p.v + r.v * 0 }
console.log(useD(((d as any).h as H).p))

let re: RegExp | null = /x/
;(re as any).h = new H()
function useR(p: P): number { re = null; const r = new P(); r.v = 4; return p.v + r.v * 0 }
console.log(useR(((re as any).h as H).p))

let arr: number[] | null = [1]
;(arr as any).h = new H()
function useA(p: P): number { arr = null; const r = new P(); r.v = 5; return p.v + r.v * 0 }
console.log(useA(((arr as any).h as H).p))

const hs: H[] = [new H()]
let q3: Q | null = new Q()
;(q3 as any).hs = hs
function useS(p: P): number { q3 = null; hs.length = 0; const r = new P(); r.v = 6; return p.v + r.v * 0 }
console.log(useS((((q3 as any).hs) as H[])[0].p))

// a method on the bag's value that clears the binding the bag hangs off
class K {
  p: P = new P()
  run(): number { q4 = null; const r = new P(); r.v = 8; return this.p.v + r.v * 0 }
}
let q4: Q | null = new Q()
;(q4 as any).k = new K()
console.log(((q4 as any).k as K).run())
