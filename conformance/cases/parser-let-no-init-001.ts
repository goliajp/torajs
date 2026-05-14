// P-PARSE.8 — `let x;` (no initialiser) per ES spec §14.3.1:
// the binding is created with value `undefined`. Pre-fix
// tora's check.rs hard-rejected the Expr::Uninit placeholder
// with 'let binding declared without initializer and never
// assigned in scope'. Test262's language/eval-code/* and
// several function-redeclaration cases use this pattern; the
// binding is declared early then maybe assigned inside an
// eval / cond branch the desugar can't statically resolve.
//
// Implementation:
// * check.rs: Expr::Uninit returns Type::Null (the closest
//   existing shape to spec's `undefined`; once P1 lands real
//   Type::Undefined the arm flips to that).
// * ssa_lower: Expr::Uninit lowers to Operand::ConstPtrNull
//   so downstream load / compare paths see a consistent
//   Null-like value.
//
// Subset notes:
// * `typeof x` of an uninit binding returns "object" in tora
//   (typeof null) but bun says "undefined" — substrate diff
//   that flips in P1.
// * `x === null` returns true in tora vs false in bun —
//   same root cause.
// * Multi-decl `let p, q, r;` followed by individual assigns
//   doesn't work (desugar_uninit_let only handles single-
//   decl LetDecl, not the Stmt::Multi shape that multi-let
//   produces). Single-decl + assign works because the desugar
//   pass rewrites `let y; y = 99` into `let y = 99` before
//   typecheck sees Uninit.
// * Test262's annexB/language/eval-code/* cases that hit
//   this pattern depend on eval() to do the actual assignment,
//   so the fix here doesn't unblock those cases — eval is
//   substrate-deep, never planned for AOT. The fix is still
//   the right direction: typecheck should never reject a
//   spec-valid `let x;` shape.

// Single-decl let-no-init followed by assign — desugar-rewrite
// happy path.
let y;
y = 99
console.log(y)                               // 99

// Typed let-no-init followed by assign of matching type.
let b: number;
b = 42
console.log(b)                               // 42

// Bare let-no-init that's never assigned — no longer a
// type-error reject. The binding is Null at runtime; Null/
// Nullable comparisons work as expected.
let bare: number | null;
console.log(bare === null)                   // true

// Regression: let-with-init still works.
let init = 7
console.log(init)                            // 7

let typedInit: string = "hi"
console.log(typedInit)                       // hi
