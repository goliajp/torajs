// rotation 507 — a throwing OVERRIDE reached through the `__dispatch_`
// vtable lane must propagate to the caller's handler: the stub's own
// name is never a may-throw fn (its body only forwards to the base), so
// the lane keyed nothing and the statement after the call ran with a
// zero result. Shapes: the throw through a base-typed binding, a
// base-typed param, an array element, a non-throwing sibling on the
// same slot, and a throw that a finally observes.
class Base {
  id: number;
  constructor(id: number) {
    this.id = id;
  }
  score(): number {
    return this.id;
  }
}
class Other extends Base {
  score(): number {
    if (this.id === 13) throw new Error("thirteen");
    return -this.id;
  }
}
class Quiet extends Base {
  score(): number {
    return this.id * 2;
  }
}
function viaParam(b: Base): number {
  return b.score() + 1;
}
const c: Base = new Other(2);
console.log(c.score());
try {
  const e: Base = new Other(13);
  console.log(e.score());
  console.log("unreachable");
} catch (e) {
  console.log("caught", (e as Error).message);
}
try {
  console.log(viaParam(new Quiet(5)));
  console.log(viaParam(new Other(13)));
  console.log("unreachable");
} catch (e) {
  console.log("caught param", (e as Error).message);
}
const all: Base[] = [new Base(1), new Quiet(2), new Other(3), new Other(13), new Other(4)];
let total = 0;
for (const x of all) {
  try {
    total += x.score();
  } catch (e) {
    console.log("caught elem", (e as Error).message, "at", x.id);
  } finally {
    console.log("after", x.id);
  }
}
console.log(total);
