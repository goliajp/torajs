// Rotation 410 — §15.7.14 step 5: a value-shaped heritage that is not
// a constructor (and not null) throws TypeError at class-DEFINITION
// time, before any member evaluates.
function probe(tag: string, v: any) {
  try {
    const B: any = v;
    class K extends B {}
    console.log(tag, "no-throw");
  } catch (e: any) {
    console.log(tag, "throw", e.constructor.name);
  }
}
probe("arrow", () => {});
probe("asyncfn", async function () {});
probe("number", 42);
probe("object", {});

// a prototype FIELD does not make a constructor — and the throw is at
// definition, not at `new`
const fake: any = { prototype: {} };
let defined = false;
try {
  class K extends fake {}
  defined = true;
  console.log("defined", defined);
} catch (e: any) {
  console.log("define-throw", e.constructor.name);
}
console.log("end");
