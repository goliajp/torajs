// The fn-expr `this` promote reads the STORE RECEIVER's shape, and it
// was reading it through an `as any` wrapper without peeling: the
// bare `C.prototype.m = function () { …this… }` promoted while
// `(C.prototype as any).m = …` took the honest reject. That cast is
// not decoration — it is the spelling a TS program has to use to hang
// a name the declared prototype type never had.

class C {
  x = 7
}
;(C.prototype as any).mm = function () {
  return this.x
}
console.log((new C() as any).mm())

// The chain root, reached the same way (and the shape 521-06's call
// consult exists for).
;(Object.prototype as any).nn = function () {
  return this.x
}
console.log(({ x: 1 } as any).nn())

// A binding the program declared `any`.
const a: any = { x: 2 }
;(a as any).f = function () {
  return this.x
}
console.log(a.f())

// An object literal binding, keyed by a name the literal never
// declared — the expando lane, whose census is keyed by binding name
// and so had to peel too.
const lit = { x: 3 }
;(lit as any).g = function () {
  return this.x
}
console.log((lit as any).g())

// Nested casts peel to the same receiver.
;((C.prototype as any) as any).oo = function () {
  return this.x * 2
}
console.log((new C() as any).oo())

// `this.<any field> = fn` inside a constructor, cast the same way.
class D {
  m: any
  constructor() {
    ;(this as any).m = function () {
      return 11
    }
  }
}
console.log((new D() as any).m())

// A non-`any` cast is NOT peeled: the lowering only guarantees the
// any lane for the widening one, so that spelling keeps the loud
// reject rather than a promoted body nothing shifts argv for.
