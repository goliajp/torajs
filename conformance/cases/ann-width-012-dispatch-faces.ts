// W3 chunk 2.6a (F5) — dispatch-face width negotiation. Three
// dispatch faces read a callee signature the width analysis never
// negotiated, so an f64-possible return (or param) silently
// bit-punned through an i64 face. All shapes here trigger on plain
// division — live silent-wrongs independent of the S9 mul flip.
//
// face 1: struct-field-fn dispatch — `widen_struct_fields` skipped
// FnSig/Closure fields, and the ObjectLit fill never wired the
// field's __ret/__p{i} projections onto the resident fn.
type H = { half: (x: number) => number };
const h: H = { half: function(x: number): number { return x / 2; } };
console.log(h.half(7));  // 3.5

// capturing variant (Closure-typed field).
let k = 2;
type S = { scale: (x: number) => number };
const s: S = { scale: function(x: number): number { return x / k; } };
console.log(s.scale(7));  // 3.5

// fn value assigned into a field after the fill joins the same
// signature class (container_assign wiring).
const h2: H = { half: function(x: number): number { return x + 1; } };
h2.half = function(x: number): number { return x / 4; };
console.log(h2.half(10));  // 2.5

// face 2: map/reduce callback positional wiring indexed the lifted
// closure's raw param list — `__env` at slot 0 shifted the acc/elem
// edges off by one (reduce acc feedback landed on the env pointer).
let xs: number[] = [1.5, 2.5, 3.5];
console.log(xs.reduce((a: number, x: number): number => a + x, 0));  // 7.5
let ys: number[] = [1, 2, 3];
console.log(ys.reduce((a: number, x: number): number => a + x / 2, 0));  // 3

// face 3: abstract/virtual dispatch — `Ret(__dispatch_<M>)` was an
// orphan class (the AST stub forwards to the base owner's __cm,
// which an abstract base never emits), so an f64 override returned
// garbage through the vtable face. One vtable slot = one ABI: the
// dispatcher and every owner share the signature class.
abstract class Shape2 {
  abstract h(): number;
  show(): number { return this.h(); }
}
class C2 extends Shape2 {
  h(): number { return 7 / 2; }
}
let v: Shape2 = new C2();
console.log(v.h());     // 3.5
console.log(v.show());  // 3.5
