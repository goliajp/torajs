// K5-2b — a static private element inside a per-evaluation family. It
// is an own entry of the ONE class object that declared it: the derived
// class object does not carry it (TypeError, not the parent's value up
// the [[Prototype]] chain), and another evaluation's is another element.
function mk(): any {
  class A {
    static #s = 7;
    static #sm() { return 1 }
    static readS() { return this.#s }
    static callS() { return this.#sm() }
    static hasS(o: any) { return #s in o }
  }
  class B extends A {}
  return [A, B]
}
const [A1, B1] = mk(), [A2, B2] = mk();
const t = (f: () => any) => {
  try { console.log(f()) } catch (e) { console.log("T", (e as any).constructor === TypeError) }
};
t(() => A1.readS());
t(() => B1.readS());
t(() => A1.callS());
t(() => B1.callS());
t(() => A1.hasS(B1));
t(() => A1.hasS(A1));
t(() => A1.hasS(A2));
t(() => A1.readS.call(A2));
t(() => A1.readS.call(B1));
t(() => Object.getPrototypeOf(B1) === A1);

// The family's names also bound at top level: the child is judged
// before its own collision rename, and must not count these readers.
function mk2(): any {
  class P { static #k = 3; static k() { return this.#k } tag() { return "P" } }
  class Q extends P { tag() { return "Q>" + super.tag() } }
  return [P, Q]
}
const [P, Q] = mk2();
const [P2, Q2] = mk2();
t(() => P.k());
t(() => Q.k());
t(() => new Q().tag());
t(() => [new Q() instanceof P, new Q() instanceof P2, new Q2() instanceof P2, Q !== Q2]);

// Top-level classes: the same own-entry rule through an `any` receiver.
class T { static #z = 9; static z(this: any) { return this.#z } }
class U extends T {}
t(() => T.z());
t(() => (U as any).z());
