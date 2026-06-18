// Class getter return-type inference for complex bodies per ES §10.1.7.
// Extends `check-class-getter-return-inf-001` (which covered bare literals
// and `return this.<field>`): the inference now recursively walks BinOp
// / Unary expressions so getters with arithmetic / comparison / string-
// concat bodies type-check without an explicit `: T` annotation.
//
// Implementation: `infer_getter_return_ann` in desugar_classes_emit.rs
// now delegates to a recursive `infer_expr_ty_in_getter` helper. The
// helper folds:
//   * Comparison / loose-eq BinOps          → boolean
//   * Logical `&&` / `||`                   → left operand's inferred type
//   * Arithmetic (non-Add) + bitwise + shift → number
//   * `+` (overloaded)                       → string if either operand
//                                              is string, else number
//   * Unary `-` / `~` / `+`                  → number
//   * Unary `!`                              → boolean
// Any unmodelled shape (Call, member-of-member, …) falls back to None,
// preserving the old `Void`-default behaviour for non-getter-style bodies.

class Box {
  x: number;
  label: string;
  constructor(v: number, l: string) {
    this.x = v;
    this.label = l;
  }
  // arithmetic body → number
  get doubled() {
    return this.x * 2;
  }
  // arithmetic mixed with literal → number
  get plus10() {
    return this.x + 10;
  }
  // string concat — string + this.<field number> → string
  get tag() {
    return "B-" + this.x;
  }
  // string concat both sides string
  get fullLabel() {
    return this.label + "-end";
  }
  // comparison → boolean
  get isPositive() {
    return this.x > 0;
  }
  // unary not on field → boolean (number != 0 coerce)
  get isZero() {
    return this.x === 0;
  }
  // bitwise → number
  get masked() {
    return this.x & 7;
  }
  // unary neg → number
  get negated() {
    return -this.x;
  }
}

const b = new Box(5, "box");
console.log(b.doubled);    // 10
console.log(b.plus10);     // 15
console.log(b.tag);        // B-5
console.log(b.fullLabel);  // box-end
console.log(b.isPositive); // true
console.log(b.isZero);     // false
console.log(b.masked);     // 5
console.log(b.negated);    // -5
