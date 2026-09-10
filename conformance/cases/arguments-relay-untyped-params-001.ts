// A `__forward_` relay's boxed adapter enters its head-less argv
// target directly with the adapter's own argc / argv (rotation 625).
// The target was looked up by stripping the relay's `__forward_`
// prefix — the TEMPLATE name — so a target with an untyped parameter
// (an implicit generic, `function f(a)`: the plain JavaScript
// spelling) matched nothing: the monomorph pass had retargeted the
// relay's forwarding call to the all-`any` clone, which lives under
// its own name. The adapter fell back to the relay body, whose
// forwarding call packs the declared parameters only, and every
// escape route answered `arguments.length` as the declared arity:
// `f(42, ...arr)` said 1 where bun says 4, `(f as any)(1, 2, 3)` said
// 1. The target is read off the forwarding call now, retarget
// included. Typed twins (`g(a: number)`) never had the gap and are
// here as the control. (`[f][0](…)` — a typed closure element call,
// the direct route — is the recorded window of rotation 625, not
// this knife's, and is left out.)
const arr = [2, 3];
function f(a) { return [a, arguments.length, arguments[3]]; }
console.log(f(42, ...[1], ...arr), f(42, 1, ...arr), f(...arr));
console.log((f as any)(1, 2, 3));
const h: any = f;
console.log(h(5), h(), h(1, 2, 3, 4, 5));
function two(a, b) { return [a, b, arguments.length, arguments[2]]; }
console.log(two(...arr), two(1, ...arr));
console.log((two as any)(9), (two as any)(1, 2, 3, 4));
function s(a) { return [a, arguments.length]; }
console.log(s(..."abc"), (s as any)("x", "y"));
function g(a: number) { return [a, arguments.length, arguments[3]]; }
console.log(g(42, ...[1], ...arr), (g as any)(1, 2, 3));
