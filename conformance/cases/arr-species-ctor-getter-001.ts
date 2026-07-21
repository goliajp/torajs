// RFC 20260721-array-proto-cluster 刀 10 G5a — the species family's
// Get(O, "constructor") runs an ACCESSOR entry's getter (§9.4.2.3
// step 3): a poisoned getter's throw is observable in concat /
// filter / map / slice / splice; a benign getter's answer takes the
// same step 5-7 classification as a data entry.

function poisoned(): any[] {
  const a: any[] = [1, 2];
  Object.defineProperty(a, "constructor", {
    get: function (): any {
      throw new Error("poison");
    },
  });
  return a;
}

function tryOp(name: string, fn: () => void): void {
  try {
    fn();
    console.log(name, "no-throw");
  } catch (e: any) {
    console.log(name, e.message);
  }
}

tryOp("concat", () => { poisoned().concat(); });
tryOp("slice", () => { poisoned().slice(0); });
tryOp("map", () => { poisoned().map((x: any) => x); });
tryOp("filter", () => { poisoned().filter((x: any) => true); });
tryOp("splice", () => { poisoned().splice(0, 1); });

// benign getter: answers undefined -> default Array product.
const b: any[] = [3, 4];
let calls = 0;
Object.defineProperty(b, "constructor", {
  get: function (): any {
    calls += 1;
    return undefined;
  },
});
const r = b.slice(0);
console.log("benign:", calls >= 1, Array.isArray(r), r.length);
