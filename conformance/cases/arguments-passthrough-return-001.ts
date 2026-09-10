// A pass-through return of an `arguments` element — `return cond ?
// arguments[1] : a`, the default-argument idiom — hands the element
// box back through a ternary / nullish / sequence / `as` arm, which
// the return lowering's root retain cannot see through. Every face
// refused such a body and it fell to the declared-params builder:
// `ret(5, 6)` answered undefined where bun answers 6, and the value
// escape died as "unknown function". The return is bound to a local
// first now (`arguments_object_return_passthrough`), and the body
// takes whichever face fits it.
const arr = [2, 3];
function ret(a: number) { return arguments.length > 1 ? arguments[1] : a; }
const r: any = ret;
console.log(ret(5), ret(5, 6), r(1), r(1, 2), ret(...arr));
function nul(a: number) { return arguments[1] ?? a; }
console.log(nul(1), nul(1, 2), nul(1, undefined), nul(1, null));
function seq(a: number) { return (a, arguments[1]); }
console.log(seq(1), seq(1, "x"));
function cast(a: number) { return arguments[1] as any; }
console.log(cast(1), cast(1, [7]));
function nested(a: number) { return a > 0 ? (arguments.length > 1 ? arguments[1] : "one") : "neg"; }
console.log(nested(1), nested(1, "two"), nested(-1, "x"));
function strs(a: string) { return arguments.length > 1 ? arguments[1] : a; }
const s1 = strs("ab" + "cd", "ef" + "gh");
const s2 = strs("ab" + "cd");
console.log(s1, s2, s1.length, s2.length);
function inloop(a: number) { for (let i = 0; i < 3; i++) { if (i === 2) { return arguments.length > 1 ? arguments[1] : i; } } return -1; }
console.log(inloop(0), inloop(0, 9));
