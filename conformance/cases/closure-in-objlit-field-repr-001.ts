// A struct fn-field slot is Closure-repr (`__cls(P)->R`) in every
// lane; a bare-fn-ptr slot (`__fn(`) would CallIndirect into the
// closure's env header (SIGBUS). Four sites mint a field ann, and
// only two of them retagged: named TypeDecl/ClassDecl fields and the
// parser's syntax-minted `__inlobj(`. The other two shipped the bare
// repr — the return-type inferrer's `__inlobj(` and the generic-mono
// `__struct(` — so an object literal holding a closure crashed as
// soon as its type came from inference rather than from source.
//
// Store-site mirror: with the slot uniformly Closure-repr, a bare
// top-level FnDecl stored into an UNTYPED object literal needs the
// `__forward_<fn>` wrap too (it previously rejected the raw fn ptr
// with "value is not a function"), and the forwarder must publish its
// full fn-shaped ann — otherwise the field ann reads back the fn's
// RETURN type as if it were the fn type.

// inferred return type — closure with no capture
function mkArrow() {
  return { f: () => 7 };
}
console.log(mkArrow().f());

// inferred return type — capturing closure (env block is live)
function mkCounter() {
  let c = 0;
  return {
    bump: () => {
      c = c + 1;
      return c;
    },
  };
}
const ctr = mkCounter();
console.log(ctr.bump(), ctr.bump(), ctr.bump());

// inferred return type — bare top-level FnDecl into the field
function top(): number {
  return 9;
}
function mkTopFn() {
  return { g: top };
}
console.log(mkTopFn().g());

// inferred return type — shorthand method
function mkMethod() {
  return {
    m() {
      return 3;
    },
  };
}
console.log(mkMethod().m());

// generic instantiation routes the struct through `type_to_ann`
function id<T>(x: T): T {
  return x;
}
console.log(id({ f: () => 11 }).f());
console.log(id({ g: top }).g());

// nested object literal, inferred both levels
function mkNested() {
  return { inner: { f: () => 13 } };
}
console.log(mkNested().inner.f());

// the two lanes that already worked — pinned against regression
function mkAnnotated(): { f: () => number } {
  return { f: () => 17 };
}
console.log(mkAnnotated().f());

const atTopLevel = { f: () => 19, g: top };
console.log(atTopLevel.f(), atTopLevel.g());

type Handlers = { onTick: () => number };
const declared: Handlers = { onTick: top };
console.log(declared.onTick());

// a Closure-repr slot is mutable: reassigning a capturing closure over
// a slot that was born holding a bare fn must keep dispatching right
function mkSwappable() {
  return { h: top };
}
const sw = mkSwappable();
console.log(sw.h());
let bias = 100;
sw.h = () => bias + 1;
console.log(sw.h());
