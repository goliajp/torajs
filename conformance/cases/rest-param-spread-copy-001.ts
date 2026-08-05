// A rest parameter is a FRESH array (ES §10.4.2, CreateArrayFromList).
//
// The single-spread call shape `f(req…, ...src)` used to hand `src`
// straight through as the rest param — a shortcut that made the rest
// param an alias of the caller's array, so a callee that pushed to it
// mutated the caller's. It also made `f(..."xyz")` a type error, since
// a String is not an Array(String) — but it IS iterable, and only the
// pass-through cared about the difference.

// the aliasing half: the callee must not be able to reach `arr`
function push99(...xs: number[]) {
  xs.push(99);
  return xs.length;
}
const arr = [1, 2, 3];
console.log(push99(...arr), arr.length, arr.join(","));

// mutation through other means is equally invisible to the source
function clobber(...xs: number[]) {
  xs[0] = 42;
  return xs.join(",");
}
const src2 = [1, 2];
console.log(clobber(...src2), src2.join(","));

// the string half: spread of a string is its code POINTS
function join(...xs: string[]) {
  return xs.length + ":" + xs.join("|");
}
console.log(join(..."xyz"));
console.log(join(..."👋a"));
console.log(join(..."héllo"));

// shapes that must not move
function sum(...ns: number[]) {
  return ns.reduce((a, b) => a + b, 0);
}
console.log(sum(...[1, 2, 3]));
console.log(sum());
console.log(sum(...[]));

function tag(p: string, ...rest: string[]) {
  return p + ":" + rest.join("|");
}
console.log(tag("a", ...["b", "c"]));
console.log(tag("a"));

// spread that is not the only rest element still builds its own array
function mix(...xs: number[]) {
  return xs.join(",");
}
const src = [7, 8];
console.log(mix(...src, 9));
console.log(mix(1, ...src));
console.log(src.join(","));

// a delegating wrapper — the shape the shortcut was written for
function fwd(...xs: number[]) {
  return sum(...xs);
}
console.log(fwd(4, 5, 6));

// two hops, so a shared array would show up as doubled mutation
function outer(...xs: number[]) {
  return push99(...xs) + "/" + xs.length;
}
const base = [1, 2];
console.log(outer(...base), base.length);
