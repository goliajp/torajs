// P4.5 — `new.target` meta-property (spec §13.3.10). Evaluates to
// the class function used at the `new` site. Inside a subclass ctor
// invoked via `super()`, the parent ctor sees the derived class
// (NOT the static ctor owner). Outside any ctor → `undefined`.
//
// Implementation:
// - parser recognizes `new` `.` `target` → Expr::NewTarget
// - desugar_classes adds a hidden `__new_target: any` param to
//   every `__cm_<C>__ctor`
// - factory `__new_<C>(...)` passes `__torajs_my_class_ref("<C>")`
//   (intercepted by ssa_lower → class_get(<tag>)) as the
//   __new_target arg
// - `super()` forwards the current ctor's __new_target through to
//   the parent ctor (Pass 1.5 rewrite)
// - ssa_lower's Expr::NewTarget arm loads `__new_target` from
//   self.locals when present (ctor body), else emits ANY_UNDEF
// - `__class_<NAME>` Ident references from inside any fn body
//   route through the runtime classes-by-tag side table (Phase A1
//   rewrite renamed user class names; the resolution happens at
//   SSA-layer ident lookup via class_name_to_tag → class_get)
//
// Runtime side table:
// - `__torajs_classes_by_tag[]` populated at module init via
//   `__torajs_class_register` (parallel to proto_register)
// - `__torajs_class_get(tag)` returns the registered __class_<C>
//   any-box with rc-bumped ownership

class A {
  constructor() {
    const t: any = new.target;
    console.log(t === A);     // line varies
    console.log(t.name);      // class name string
  }
}

class B extends A {
  constructor() {
    super();
    const t: any = new.target;
    console.log(t === B);
    console.log(t === A);
    console.log(t.name);
  }
}

class C extends B {
  constructor() {
    super();
    const t: any = new.target;
    console.log(t === C);
    console.log(t.name);
  }
}

// 1. `new A()` — A's ctor sees t = A
new A();   // true / A

// 2. `new B()` — A's ctor (via super) sees t = B; B's ctor sees t = B
new B();   // false / B  (A ctor)
           // true / false / B  (B ctor)

// 3. `new C()` — A's, B's, C's ctors all see t = C
new C();   // false / C  (A ctor)
           // false / false / C  (B ctor)
           // true / C  (C ctor)

// 4. Outside any ctor → undefined
function notACtor(): void {
  const t: any = new.target;
  console.log(t === undefined);
}
notACtor();
