// `c.v++` / `--o.a` / `a.length++` on a possibly-undefined receiver:
// the old value is GetValue(lhs) (§13.4.2.1 / §13.4.4.1), which is
// §7.1.20 ToObject on an absent base — a TypeError before any load.
//
// The member read lane let these receivers through (they decay to the
// underlying type for member lookup, and the update expression's
// checker asks that lane), but the update expression's lowering loaded
// the field straight off the receiver with no guard: an absent value
// was a SIGSEGV. The guard is the one every other member lane uses.
//
// Written out directly, never through a helper (r687).

class C {
  v: number = 3;
}

function maybeC(): C | undefined {
  return Math.random() < 2 ? new C() : undefined;
}
function noneC(): C | undefined {
  return undefined;
}
function noneO(): { a: number } | undefined {
  return undefined;
}
function noneA(): number[] | undefined {
  return undefined;
}

function main(): void {
  const c: C | undefined = maybeC();
  c.v++;
  ++c.v;
  console.log(c.v);

  const gc: C | undefined = noneC();
  try {
    gc.v++;
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  const go: { a: number } | undefined = noneO();
  try {
    --go.a;
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  const ga: number[] | undefined = noneA();
  try {
    ga.length++;
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
}

main();
