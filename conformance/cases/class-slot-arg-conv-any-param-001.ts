// An argument reaching an unannotated (`any`) parameter has to be
// boxed like it is on every other call lane. The vtable-slot lane
// handed its arguments over verbatim, so an i64 arrived as raw bits
// and the body read it back as `null` — the same defect the
// sibling-class lane was fixed for, one lane over.
class A {
  f(x) {
    console.log("A", x)
    return x
  }
}
class B extends A {
  f(x) {
    console.log("B", x)
    return x
  }
}
const xs: A[] = [new A(), new B()]
for (const x of xs) console.log(x.f(2))

// a string takes the same trip
class S {
  g(v) {
    return v
  }
}
class T extends S {
  g(v) {
    return v
  }
}
const ss: S[] = [new S(), new T()]
for (const s of ss) console.log(s.g("hi"), s.g(true))
