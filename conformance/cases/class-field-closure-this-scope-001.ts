// `__this` names a different object in every function, so the arrow
// written into a class field must be typed by ITS OWN class. The table
// that answered "what does this receiver hold" was flat and keyed by
// bare name, so one `__this` entry served the whole program and the
// class declared LAST won every lookup.
//
// The result was a wrong ANSWER, not a refusal: an earlier class's
// field arrow lost its declared type, was lifted with an `any`
// parameter while the call site passed an unboxed number, and the call
// came back 0. Declaring the second class first hid it.

// A second class — an empty one is enough to have its own `__this`.
class Inner {
  f: (a: number) => number = (a) => a * 3
}
class Empty {}

const i = new Inner()
const g = i.f
console.log(g(2), g(10), g(0))

// The worst face: the other class declares a field of the SAME name
// with a different type, so the arrow took that type instead of `any`
// and was lifted `(a: string) -> string`.
class Num {
  f: (a: number) => number = (a) => a + 1
}
class Str {
  f: (s: string) => string = (s) => s + "!"
}
const n = new Num()
const sn = new Str()
const gn = n.f
const gs = sn.f
console.log(gn(5), gs("hi"))

// The argument really arrives, rather than a garbage value that
// happens to print.
class Logged {
  f: (a: number) => number = (a) => {
    console.log("arg", a, typeof a)
    return a * 2
  }
}
class After {}
const lg = new Logged()
const glg = lg.f
console.log(glg(21))

// Three classes: every one of them keeps its own field type.
class A1 {
  f: (a: number) => number = (a) => a + 100
}
class A2 {
  f: (a: number) => number = (a) => a + 200
}
class A3 {
  f: (a: number) => number = (a) => a + 300
}
const a1 = new A1()
const a2 = new A2()
const a3 = new A3()
const f1 = a1.f
const f2 = a2.f
const f3 = a3.f
console.log(f1(1), f2(1), f3(1))

// Assigned by the constructor rather than a field initializer, and by a
// method — both are `__this.f = <arrow>` after desugar.
class ByCtor {
  f: (a: number) => number
  constructor() {
    this.f = (a) => a * 5
  }
}
class ByCtorAfter {}
const bc = new ByCtor()
const gbc = bc.f
console.log(gbc(4))

class ByMethod {
  f: (a: number) => number = (a) => a
  install() {
    this.f = (a) => a * 7
  }
}
class ByMethodAfter {}
const bm = new ByMethod()
bm.install()
const gbm = bm.f
console.log(gbm(3))

// A capturing arrow in a field: the env is separate from the typing,
// and both have to survive.
class Capt {
  k: number = 9
  f: (a: number) => number
  constructor(base: number) {
    this.f = (a) => a + base
  }
}
class CaptAfter {}
const cp = new Capt(1000)
const gcp = cp.f
console.log(gcp(7), cp.k)

// Declaring the other class FIRST was the shape that already worked —
// it must keep working.
class Before {}
class Later {
  f: (a: number) => number = (a) => a - 1
}
const lt = new Later()
const glt = lt.f
console.log(glt(10))

// A field of a nested class instance, reached through the outer object.
class Leaf {
  f: (a: number) => number = (a) => a * 11
}
class Holder {
  leaf: Leaf = new Leaf()
}
class HolderAfter {}
const hd = new Holder()
const inner = hd.leaf
const gl = inner.f
console.log(gl(2))
