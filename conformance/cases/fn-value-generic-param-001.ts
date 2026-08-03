// Generic-param call-arg wrap axis (rotation 291): a fn-name argument
// matching a TypeVar-annotated param (`same<T>(a: T, b: T)`)
// instantiates the generic at Any — the argv slot any-boxes, which a
// raw FnSig can't. The canonical `__forward_*` cell keeps identity
// across wrap sites, so `===` faces still answer true.

function same<T>(a: T, b: T): boolean {
  return a === b;
}

function probe() {
  return 42;
}

// fn-name on both generic slots — identity must hold
console.log(same(probe, probe));
// fn-name against an any-bound copy of itself
const viaAny: any = probe;
console.log(same(viaAny, probe));
// unrelated values keep answering through the same generic
console.log(same(1, 2));
console.log(same("x", "x"));
// the wrapped cell still calls
const held: any = probe;
console.log(held());
