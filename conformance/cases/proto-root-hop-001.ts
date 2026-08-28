// 517-07 — §10.1.8.1 OrdinaryGet step 4: the walk does not end at the
// receiver's family prototype. Every non-dynobj lane stopped at its own
// (Arr at Array.prototype, a wrapper at its tag singleton, a closure at
// Function.prototype), so a property the program installed on
// Object.prototype was unreachable from all of them while `({}).foo`
// answered it — the dynobj lane got the hop in 517-02 and the others
// did not.
(Object.prototype as any).foo = 5;

const a: any = [1, 2];
console.log("arr", a.foo);

class C {
  x = 1;
}
const o: any = new C();
console.log("struct", o.foo);

const s: any = "ab";
console.log("string", s.foo);

const n: any = 7;
console.log("number", n.foo);

const f: any = () => 1;
console.log("closure", f.foo);

const p: any = Promise.resolve(1);
console.log("promise", p.foo);

const d: any = { z: 1 };
console.log("dynobj", d.foo);

const m: any = new Map();
console.log("map", m.foo);

// an own property still shadows the root
const a2: any = [3];
a2.foo = 9;
console.log("shadow", a2.foo);

// and the family's own surface still wins over the root
(Object.prototype as any).length = 99;
console.log("family-wins", a.length);

// absent is still absent
console.log("absent", a.nope);
