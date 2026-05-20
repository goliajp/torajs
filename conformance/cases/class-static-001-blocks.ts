// P8.3 — class static blocks (ES2022 §15.7.10). Each `static { ... }`
// inside a class body is one of the class's static initialization
// steps, evaluated in source order interleaved with static field
// initializers. desugar_classes emits each block as a top-level
// `function __sb_<C>__<idx>(): void { body }` plus a top-level
// `Stmt::Expr(Call(...))` pushed into `static_field_inits` at the
// block's source-order position so the runtime order matches the
// spec exactly:
//
//   - `static x = 1` → LetDecl __sf_C__x
//   - `static { ... }` → FnDecl __sb_C__<idx> + top-level Call
//
// Both go into the same `static_field_inits` accumulator that is
// PREPENDED to ast.stmts (before any user top-level code) so the
// class is fully initialized by the time downstream code runs.

// 1) Single static block — body is invoked exactly once when the
//    class is evaluated; side effects observable on stdout.
class A {
  static {
    console.log('A:static-block-runs')
  }
}
void A

// 2) Interleaved field + block (source order). Block reads the
//    static field declared above it via ClassName.field — exercises
//    the `static_member_rewrites` table (B.x → __sf_B__x).
class B {
  static x: number = 1
  static {
    console.log(B.x)
  }
  static y: number = 2
  static {
    console.log(B.y)
  }
}
void B

// 3) Multi-block + cross-reference. Later blocks see the cumulative
//    state of earlier fields, including reads computed from multiple
//    earlier slots.
class C {
  static {
    console.log('C:before')
  }
  static a: number = 10
  static {
    console.log('C:after-a')
  }
  static {
    console.log('C:before-b')
  }
  static b: number = 20
  static {
    console.log(C.a + C.b)
  }
}
void C

// 4) Block-only class (no static fields). Verifies the Field-only
//    filter on static_member_rewrites does not break a class whose
//    only static initialization step is a block.
class D {
  static {
    console.log('D:block-only')
  }
}
void D
