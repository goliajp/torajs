// `return x as any` must judge ownership by the INNER read. The cast
// is a value-transparent pass-through for a heap source, so judging
// the `As` node skipped both ownership legs of the return path at
// once: no retain for a borrowed read, and no moved mark for an owned
// local -- so the fn-exit drop walk released the cell it had just
// handed the caller. The churn loop below reuses that page, which is
// why a stale read shows up as the wrong text instead of a crash.
function fresh(): any {
  const s = "ab" + "cd";
  return s as any;
}

function elem(xs: string[]): any {
  return xs[0] as any;
}

const a = fresh();
const b = elem(["kept", "other"]);

for (let i = 0; i < 200; i++) {
  const junk = "x" + i;
  if (junk === "never") console.log(junk);
}

console.log(a);
console.log(b);
