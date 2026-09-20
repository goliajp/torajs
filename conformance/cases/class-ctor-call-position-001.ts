// §10.2.1 step 2 — a class object HAS [[Call]]; what that [[Call]]
// does is the question. tr models a class as a dynobj, so the
// any-lane dispatch used to fall through to "value is not a
// function", which is a false statement about the program: the value
// IS a function (`typeof` says so), it just refuses this call.
//
// The refusal existed, in exactly one place: the bound-call path. So
// `C.bind(null)()` named the rule and a plain `C()` did not.
class U {
  x: number = 1;
}
const Uc: any = U;
function attempt(f: any): string {
  try {
    const r = f();
    return "ran:" + String(r);
  } catch (e: any) {
    return e && e.constructor ? e.constructor.name : "?";
  }
}
console.log("plain  ", attempt(() => Uc()));
console.log("bound  ", attempt(() => Uc.bind(null)()));
console.log("call   ", attempt(() => Uc.call(null)));
console.log("apply  ", attempt(() => Uc.apply(null, [])));
console.log("proxy  ", attempt(() => new Proxy(Uc, {})()));

// §20.5.1.1 / §20.5.6.2 — the Error hierarchy is the one family tr
// injects as classes, and every member of it is callable with `new`'s
// behaviour. `new E` first so the factory adapter is synthesized:
// reaching a class through a runtime value is pay-for-use, and a
// call position alone does not arm it yet.
const E: any = Error;
const seed: any = new E("seed");
console.log("seed   ", seed.message);
console.log("Error  ", E("m").message, E("m") instanceof Error);
const T: any = TypeError;
console.log("TypeErr", T("m").message, T("m") instanceof TypeError);
console.log("nested ", attempt(() => E("q").message));
