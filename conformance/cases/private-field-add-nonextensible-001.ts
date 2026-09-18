// PrivateFieldAdd / PrivateMethodOrAccessorAdd on a NON-EXTENSIBLE
// object is a TypeError (proposal nonextensible-applies-to-private,
// step 1 of both): the return-override trick may not stamp a frozen
// object, and nothing is left on it. bun 1.4.2 does not implement the
// proposal, so the `.expected` beside this file carries the spec's
// answers, checked by hand against the proposal's two steps.
const t = (f: () => any) => { try { console.log(f()) } catch (e) { console.log("T", (e as any).constructor === TypeError) } };
class Trojan { constructor(obj: any) { return obj } }
class F extends Trojan { #v; constructor(obj: any) { super(obj); this.#v = 42 } static v(obj: any) { return obj.#v } static has(obj: any) { return #v in obj } }
class M extends Trojan { constructor(obj: any) { super(obj) } #m() { return 7 } static m(obj: any) { return obj.#m() } static has(obj: any) { return #m in obj } }
const open: any = {}, sealed: any = Object.preventExtensions({ a: 1 }), frozen: any = Object.freeze({});
t(() => F.v(new F(open)));
t(() => new F(sealed));
t(() => F.has(sealed));
t(() => new F(frozen));
t(() => M.m(new M(open)));
t(() => new M(sealed));
t(() => M.has(sealed));
// The runtime-value lane (a class expression that captures) installs
// its fields the same way and refuses the same receivers.
function mk(seed: number): any {
  return class extends Trojan { #s = seed; constructor(obj: any) { super(obj) } static s(o: any) { return o.#s } }
}
const K = mk(3);
t(() => K.s(new K({})));
t(() => K.s(sealed));
