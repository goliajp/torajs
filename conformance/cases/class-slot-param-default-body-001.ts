// A class method's default used to be supplied by the CALL SITE:
// `apply_default_args` pads by method name, and only when every owner
// of that name agrees. Widening a vtable slot is exactly what makes
// them disagree, so the row that owns the default stopped receiving it
// — which is why such a slot was refused outright. The default now
// moves into the body as an `if (y === undefined) y = 5` guard, the
// way a plain function's already does.
class A {
  f(x) {
    return x
  }
}
class B extends A {
  f(x, y = 5) {
    return x + y
  }
}
const xs: A[] = [new A(), new B()]
for (const x of xs) console.log(x.f(1))

// an explicitly supplied argument wins over the default
for (const x of xs) console.log(x.f(1, 100))

// an explicit `undefined` still fires it (section 10.2.1.4) — only a
// callee-side guard can see that, the pad fills missing positions only
for (const x of xs) console.log(x.f(1, undefined))

// the same row reached without the slot, and through the any lane
console.log(new B().f(1))
const b: any = new B()
console.log(b.f(1))

// a default reading a prior parameter is evaluated in the CALLEE's
// scope, in parameter order (section 9.2)
class C {
  g(x) {
    return x
  }
}
class D extends C {
  g(x, y = x + 1) {
    return x + y
  }
}
const cs: C[] = [new C(), new D()]
for (const c of cs) console.log(c.g(4))

// three rows, each wider than the last, each with its own defaults
class E {
  m(x) {
    return x
  }
}
class F extends E {
  m(x, y = 5) {
    return x + y
  }
}
class G extends F {
  m(x, y = 9, z = 1) {
    return x + y + z
  }
}
const es: E[] = [new E(), new F(), new G()]
for (const e of es) console.log(e.m(2))

// an unrelated class declaring the same method name with a default of
// its own must not have its literal pasted into this slot's call sites
class H {
  f(p, q = 3) {
    return p * q
  }
}
console.log(new H().f(4))
