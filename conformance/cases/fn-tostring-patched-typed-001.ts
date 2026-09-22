// A typed function value's `f.toString()` folds to its source only
// while the program cannot replace the method it reaches. Here it can
// — the closure's own `toString` and Function.prototype.toString are
// both patched — so each call reads the live property.
const h = (x: string) => x;
const k = (y: number) => y + 1;
(h as any).toString = () => "patched";
console.log(h.toString(), k.toString() === String(k));
const orig = Function.prototype.toString;
(Function.prototype as any).toString = function () { return "P"; };
console.log(k.toString(), `${k}` === "P", String(k));
Function.prototype.toString = orig;
console.log(k.toString() === orig.call(k), h.toString());
function takes(f: (n: number) => number) { return f.toString(); }
console.log(takes(k) === orig.call(k));
