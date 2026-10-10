// A fn-typed binding whose slot holds a closure cell (stored into the
// heap, reassignable, or compared with ===) handed to a fn-typed
// param. Each case calls its own receiver, so no other argument marks
// that receiver's param for it.
type F = (x: number) => number;
function h(x: number): number { return x * 2; }
function inc(x: number): number { return x + 1; }
function call(f: F): number { return f(1); }
const fs: F[] = [];
const g: F = h;
fs.push(g);
console.log(call(g), fs[0](3));
function other(): number { const g: F = inc; return call(g); }
console.log(other());
const c = Number("1") > 0;
const k = c ? h : inc;
fs.push(k);
console.log(call(k), fs[1](4));

// one stored binding into several fn-typed params
function two(f: F, f2: F): number { return f(1) + f2(2); }
console.log(two(g, g));

// the callback param of a user-defined higher-order function
function mapAll(xs: number[], f: F): number[] {
  const out: number[] = [];
  for (const x of xs) out.push(f(x));
  return out;
}
console.log(mapAll([1, 2, 3], k).join(","));
const arrow = (f: F, x: number): number => f(x) + 1000;
console.log(arrow(g, 9));

// nullable and optional fn-typed params
function maybe(f: F | null): number { return f === null ? -1 : f(7); }
console.log(maybe(k), maybe(null));
function opt(f?: F): number { return f ? f(8) : -2; }
console.log(opt(g), opt());

// the same name in another scope, where it is not stored
function apply3(f: F): number { return f(3); }
function scopedPlain(): number { const g: F = inc; return g(4) + apply3(g); }
console.log(scopedPlain());

// stored into a Map
function apply5(f: F): number { return f(5); }
const m = new Map<string, F>();
const k2 = Number("0") > 0 ? h : inc;
m.set("a", k2);
console.log(apply5(k2), m.get("a")!(3));

// a mutable fn-typed slot (always a cell), reassigned
function apply6(f: F): number { return f(6); }
let cb: F = h;
console.log(apply6(cb));
cb = inc;
console.log(apply6(cb));

// the identity axis: a compared binding is the canonical cell
function apply7(f: F): number { return f(7); }
function ident(): number { const e: F = h; return e === h ? apply7(e) : 0; }
console.log(ident());
function apply8(f: F): number { return f(8); }
const ch: F = c ? h : inc;
console.log(ch === h, apply8(ch));
