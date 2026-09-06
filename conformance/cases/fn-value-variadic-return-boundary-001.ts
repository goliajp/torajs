// A rest-parameter declaration handed out through a fn-typed `return`
// has to arrive callable. `return g` is a bare identifier, not a
// closure expression, so the return-site forwarder pass's sniff — "does
// this body return something closure-producing?" — answered no for it.
// A non-variadic return survives that, because the raw code address it
// hands back is callable as it stands at the call site. A variadic call
// site is not: it packs its arguments through the boxed-dual route,
// which needs a real cell. So this one program compiled and then died
// at the call with "value is not a function", while every other
// spelling of it worked.

function g(...xs: number[]): number {
  return xs.length;
}

// the return boundary — the shape that was broken
function mk(): (...a: number[]) => number {
  return g;
}
console.log(mk()(1, 2, 3), mk()(), mk()(9));

// returned twice from the same fn, and through a branch
function mk2(flag: boolean): (...a: number[]) => number {
  if (flag) {
    return g;
  }
  return g;
}
console.log(mk2(true)(1, 2), mk2(false)(1, 2, 3, 4));

// the spellings that already worked, kept so a regression on the wrap
// shows up here rather than somewhere quieter
function take(h: (...a: number[]) => number): number {
  return h(1, 2, 3);
}
const b: (...a: number[]) => number = g;
const anyB: any = g;
const z: any[] = [g];
console.log(take(g), b(1, 2), anyB(1), z[0](1, 2, 3, 4, 5), g(1));

// a non-variadic return through the same boundary is untouched
function g2(a: number): number {
  return a;
}
function mk3(): (a: number) => number {
  return g2;
}
console.log(mk3()(7));
