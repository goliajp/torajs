// A name an inner scope rebinds is not a read of the outer function.
// The value-face rewrite for a `this`-using declaration walks the
// expression arena, which carries no scope information, so it refuses
// any name the program rebinds anywhere rather than turn a read of the
// inner binding into a read of the function.
function fn(a: any) {
  return "a=" + a + "/" + typeof (this as any);
}

function g() {
  const fn = 1;
  return fn;
}

function h(fn: number) {
  return fn;
}

const k = (fn: number) => fn;

console.log(g(), h(5), k(9));
console.log(typeof fn, fn.name);
