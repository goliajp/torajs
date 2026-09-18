// K6-2 — private FIELDS of a class in the runtime-value lane (it
// captures an outer binding, or extends a runtime value): its
// instances are plain objects, so the constructor prefix installs each
// field in the foreign store instead of writing through a brand the
// object does not have yet.
const t = (fn: () => any) => { try { console.log(fn()) } catch (e) { console.log("T", (e as any).constructor === TypeError) } };
function f(v: number): any {
  return class {
    #p = v; #q; pub = 1; #r = this.#p + 1;
    g() { return [this.#p, this.#q, this.#r] }
    set(o: any, n: number) { o.#q = n; return o.#q }
    has(o: any) { return #p in o }
  }
}
const K = f(1);
const a: any = new K(), b: any = new K();
t(() => a.g()); t(() => a.set(a, 9)); t(() => a.g()); t(() => b.g());
t(() => a.has(a)); t(() => a.has(b)); t(() => a.has({})); t(() => a.set({}, 1));
t(() => Object.keys(a).join() + "|" + JSON.stringify(a) + "|" + Object.getOwnPropertyNames(a).join());

class Base { constructor(public tag: string) {} }
function g(B: any, seed: number): any {
  return class extends B { #s = seed; constructor() { super("x"); this.#s += 1 } s() { return this.#s + ":" + (this as any).tag } }
}
function h(B: any, seed: number): any { return class extends B { #s = seed * 2; s() { return this.#s + ":" + (this as any).tag } } }
t(() => new (g(Base, 10))().s());
t(() => new (h(Base, 10))("y").s());

// A REAL class with a private field reached as a runtime-value parent:
// its constructor twin runs on the lane's plain-object `this`.
class P { v = 1; #x = 5; #y; getX() { return this.#x } setY(n: number) { this.#y = n; return this.#y } static isP(o: any) { return #x in o } }
function mk(B: any): any { return class extends B { w = 2 } }
const d: any = new (mk(P))();
t(() => [d.v, d.w, d.getX(), d.setY(3), P.isP(d), P.isP({}), d instanceof P, Object.keys(d).join()]);
t(() => new P().getX() + new P().setY(4));

// …and a per-evaluation one: the evaluation's own methods admit the instance.
function base(): any { class U { #x = 5; getX() { return this.#x } } return U }
const U1 = base(), U2 = base(), du: any = new (mk(U1))();
t(() => [du.getX(), U1.prototype.getX.call(du), du instanceof U1, du instanceof U2]);
t(() => U2.prototype.getX.call(du));
