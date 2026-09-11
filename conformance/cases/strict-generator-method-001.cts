// §11.2.2 for the two generator spellings that are METHODS. Both
// parse sites restored the enclosing strictness bit without writing
// it into the body, so neither could answer the per-function question
// §13.15.2 asks — even after the state machine learned to carry a
// directive it was handed (`strict-generator-body-001`).
//
// The two differ in where the strictness comes from: a class body is
// strict code whatever the goal (§11.2.2), while an object literal is
// not — its method is strict only when something encloses it.
//
// Oracle is node, pinned in `.expected`: bun runs a `.cts` entry with
// no strictness at all and a `.js` entry as strict throughout.
const frozen = Object.freeze({ a: 1 });

// A class generator method, in a sloppy script: strict by §11.2.2.
class C {
  *m(): any {
    frozen.a = 2;
    yield 1;
  }
}
try {
  new C().m().next();
  console.log("class-gen", "no throw");
} catch (e) {
  console.log("class-gen", (e as any).constructor.name);
}

// An object-literal generator method in sloppy code stays sloppy.
const sloppyObj = {
  *m(): any {
    frozen.a = 3;
    yield frozen.a;
  },
};
console.log("objlit-sloppy", sloppyObj.m().next().value);

function strictOuter(): void {
  "use strict";
  // The same spelling, now strict by inheritance.
  const strictObj = {
    *m(): any {
      frozen.a = 4;
      yield 1;
    },
  };
  try {
    strictObj.m().next();
    console.log("objlit-strict", "no throw");
  } catch (e) {
    console.log("objlit-strict", (e as any).constructor.name);
  }

  // A yield before the refusal, so the assignment sits in a later arm.
  const later = {
    *m(): any {
      yield 1;
      frozen.a = 5;
    },
  };
  try {
    const it = later.m();
    it.next();
    it.next();
    console.log("objlit-after-yield", "no throw");
  } catch (e) {
    console.log("objlit-after-yield", (e as any).constructor.name);
  }
}
strictOuter();
