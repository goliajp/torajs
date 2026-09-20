// §7.3.15 SetIntegrityLevel step 3-4 — "Let status be ?
// O.[[PreventExtensions]](). If status is false, return false" — and
// §20.1.2.{6,20} turn that false into the TypeError Object.freeze and
// Object.seal raise. Both begin at this step, and it is the same step
// Object.preventExtensions already performed correctly, Proxy arm and
// all. Freeze and seal went straight to the header markers instead, so
// a proxy whose trap refused was marked frozen on the proxy cell itself
// and the call answered normally.

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label + ": " + String(f()));
  } catch (e) {
    console.log(label + ": " + (e as any).constructor.name);
  }
}

function refusing(): any {
  return new Proxy({ a: 1 }, { preventExtensions() { return false; } });
}

probe("freeze-refused", () => { Object.freeze(refusing()); return "ran"; });
probe("seal-refused", () => { Object.seal(refusing()); return "ran"; });

// Control — the surface that always had this step still has it.
probe("preventExtensions-refused", () => {
  Object.preventExtensions(refusing());
  return "ran";
});

// Control — Reflect answers the boolean rather than throwing (§28.1.10),
// which is the whole difference between the two spellings.
probe("reflect-preventExtensions", () => Reflect.preventExtensions(refusing()));

// Controls — a proxy whose trap ACCEPTS goes through as before.
function accepting(): any {
  return new Proxy({ a: 1 }, { preventExtensions(t: any) { Object.preventExtensions(t); return true; } });
}
probe("freeze-accepted", () => { Object.freeze(accepting()); return "ran"; });
probe("seal-accepted", () => { Object.seal(accepting()); return "ran"; });

// Controls — an ordinary object never refuses, so nothing changed for it.
const o: any = { a: 1 };
Object.freeze(o);
console.log(Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o));
const s2: any = { b: 2 };
Object.seal(s2);
console.log(Object.isFrozen(s2), Object.isSealed(s2), Object.isExtensible(s2));
const arr: any = [1, 2];
Object.freeze(arr);
console.log(Object.isFrozen(arr));
console.log(String(Object.freeze(1 as any)), String(Object.seal("x" as any)));
