// An alias of a `var`-declared variadic closure routes its calls the
// same way the `var` itself does. The routing record refused such an
// alias because it asked the question through the spelling that TYPES
// a promoted slot, and that spelling keeps `var` out — so the call
// took a static dispatch against the fixed-prefix sig (arity 0 for a
// bare rest) and lowering refused with a K.4 ident-alias mismatch.
var gv = (...xs: number[]): number => xs.length;
const gb = gv;

function viaAlias(): number {
  return gb(1, 2, 3);
}
console.log(viaAlias(), gb(), gb(9));

// the direct flavor, which the record already admitted — it must keep
// working beside the alias
function viaVar(): number {
  return gv(1, 2);
}
console.log(viaVar(), gv());

// a chain over a var root, one hop at a time
const gc = gb;
const gd = gc;
function viaChain(): number {
  return gd(1, 2, 3, 4);
}
console.log(viaChain(), gd());

// a variadic FACE written on the `var`, over a fixed-arity closure:
// the call site is what has any arity, so it boxes exactly the same
var vf: (...xs: number[]) => number = (a: number): number => a * 2;
const fb = vf;
function viaFace(): number {
  return fb(21);
}
console.log(viaFace(), fb(21, 99));

// a string element type, and the value passed on as an argument
var vs = (...xs: string[]): string => xs.join("-");
const sb = vs;
function useIt(h: (...a: string[]) => string): string {
  return h("p", "q");
}
function viaStr(): string {
  return sb("a", "b");
}
console.log(viaStr(), sb(), useIt(sb));

// control: a non-variadic `var` source keeps its own lane
var vn = (a: number): number => a * 2;
const nb = vn;
function viaFixed(): number {
  return nb(21);
}
console.log(viaFixed(), nb(1));
