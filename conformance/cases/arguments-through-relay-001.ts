// A top-level function reached as a VALUE — through an `as any`
// cast, an any array, an object field, an `any` parameter, a
// binding — is invoked through its `__forward_` relay. The relay's
// boxed adapter now enters the target with the call's own argc and
// argv, so `arguments` past the declared parameters survives the
// hop; a fn with direct sites AND an escape leaves the static fold
// (the escape's count is a runtime value); a rest callee's direct
// call counts its packed tail at runtime; and `arguments as any` is
// a touch like any other.
function g0() { return arguments[0]; }
function gl() { return arguments.length; }
function g1(a: number) { return [a, arguments[1], arguments.length]; }
function gr(...xs: number[]) { return [xs.length, arguments.length, arguments[2]]; }
function gs() { return Array.from(arguments as any).join(","); }
function ge() { return [...arguments]; }
function gc() { const a = arguments; return a[0]; }
function gt() { return (arguments as any).length; }
console.log(g0(7), (g0 as any)(7), (g0 as any)(9, 10));
const h: any = g0; const fs: any[] = [g0]; const o: any = { f: g0 }; const o2 = { f: g0 };
console.log(h(1), (h as any)(2), fs[0](3), o.f(4), (o2.f as any)(5));
function call(f: any) { return f(6, 7); }
console.log(call(g0), call(gl), call(g1), call(gr), call(gs), call(ge), call(gc), call(gt));
console.log((gl as any)(), (gl as any)(1, 2, 3), (g1 as any)(1), (g1 as any)(1, 2, 3));
console.log((gr as any)(1, 2, 3, 4), (gs as any)("a", "b"), (gs as any)(), gt(1, 2), gs("x"));
const w = () => { function n() { return arguments[0]; } return (n as any)(11); };
console.log(w());
class K { static s() { return arguments.length; } m() { return arguments[0]; } }
const ks: any = K.s; const km: any = new K().m;
console.log(ks(1, 2), km(3), (K.s as any)(4, 5, 6));
// direct sites of one width plus an escape: the escape's count wins
function hm(a: number) { return arguments[1]; }
console.log(hm(1, 2), (hm as any)(3, 4), (hm as any)(3));
function hv(a: number) { return [a, arguments[1], arguments.length]; }
console.log(hv(1, 2), hv(3, 4), (hv as any)(5), (hv as any)(6, 7, 8));
// a rest callee's direct calls of differing width, spreads included
function rl(...xs: number[]) { return arguments.length; }
function rm(a: number, ...xs: number[]) { return [a, xs.length, arguments.length]; }
const xs = [1, 2, 3];
console.log(rl(1, 2), rl(1, 2, 3), rl(), rm(1, 2), rm(1, 2, 3, 4), rm(5), rl(...xs), rl(0, ...xs));
const fr: any = gr; console.log(fr(1, 2, 3, 4));
const fl = rl; console.log(fl(1, 2, 3));
