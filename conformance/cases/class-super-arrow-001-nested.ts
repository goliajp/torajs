// P8.4 — super.method() / super(args) inside nested arrow functions.
// Pre-A1, an unannotated expression-body arrow whose body is a Call
// expression had no inferred return type: infer_expr_ann_with bailed on
// `Expr::Call`, the lifted closure FnDecl's return_type defaulted to
// Void, and typecheck rejected `case1(): void { console.log(f()) }`
// with "return type mismatch: function expects Void, got String".
//
// Root cause was the inference gap, not the super walker — collectors
// already recurse into ArrowFn body (ast.rs:4263/4455), and
// desugar_classes in-place mutates the super-call ExprId (ast.rs:3303,
// 3335) regardless of nesting depth. After A1's fn_sigs plumbing,
// `() => super.greet()` desugars to `() => __cm_A__greet(__this)` and
// infer_expr_ann_with consults the fn_sigs table (built from non-
// `__closure_*`, non-generic top-level FnDecls including the
// synthesized `__cm_<C>__<m>`) to recover the arrow's return type.

class A {
  greet(): string { return "A.greet" }
  add(a: number, b: number): number { return a + b }
}

class B extends A {
  // 1) Bare expression-body arrow, super.method() with no args.
  case1(): void {
    const f = () => super.greet()
    console.log(f())
  }

  // 2) Block-body arrow, explicit `return super.method()`.
  case2(): void {
    const f = () => { return super.greet() }
    console.log(f())
  }

  // 3) Bare arrow calling super.method(args) with arguments — verifies
  //    the Call arm doesn't only handle zero-arg shapes.
  case3(): void {
    const f = () => super.add(2, 3)
    console.log(f())
  }

  // 4) Arrow assigned to one let then aliased to another, called
  //    through the alias. Method's outer return type is Void so this
  //    exercises the inner arrow's inference in isolation.
  case4(): void {
    const f = () => super.greet()
    const g = f
    console.log(g())
  }
}

class P {
  x: number
  constructor(n: number) { this.x = n }
}

// 5) super(args) inside a nested arrow in a subclass ctor — Pass 1.5
//    of desugar_classes collects super-calls through ArrowFn body, so
//    the in-place rewrite reaches the wrapped super() and emits the
//    `__cm_P__ctor(__this, __new_target, args)` form.
class Q extends P {
  constructor(n: number) {
    const init = () => super(n * 2)
    init()
  }
}

const b = new B()
b.case1()
b.case2()
b.case3()
b.case4()

const q = new Q(3)
console.log(q.x)
