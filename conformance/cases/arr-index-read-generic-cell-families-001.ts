// Reading past the end of an array whose elements are refcounted
// pointers answers `undefined` (ES §10.4.2.1), and so do a `find`
// miss, `at` out of range, and `pop` / `shift` on an empty one.
// Obj / Arr / Closure elements have answered this way since RFC
// 20260722 chunk B; the rest of the family — BigInt, Date, Map, Set,
// RegExp — spell it with the same immortal cell.
function line(tag: string, f: () => unknown) {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "THREW", (e as Error).name);
  }
}

const bs: bigint[] = [1n, 2n];
line("bigint-oob", () => bs[5]);
line("bigint-find", () => bs.find((x) => x > 100n));
line("bigint-at", () => bs.at(9));
const bempty: bigint[] = [];
line("bigint-pop", () => bempty.pop());
line("bigint-shift", () => bempty.shift());

const ds: Date[] = [new Date(0)];
line("date-oob", () => ds[5]);
line("date-find", () => ds.find((d) => d.getTime() > 1e15));
line("date-at", () => ds.at(9));

const ms: Map<string, number>[] = [new Map()];
line("map-oob", () => ms[5]);
line("map-find", () => ms.find((m) => m.size > 100));

const sets: Set<number>[] = [new Set()];
line("set-oob", () => sets[5]);

const res: RegExp[] = [/a/];
line("regexp-oob", () => res[5]);

// The value faces: what the answer says about itself.
line("typeof-date", () => typeof ds[5]);
line("typeof-bigint", () => typeof bs[5]);
line("typeof-map", () => typeof ms[5]);
line("typeof-regexp", () => typeof res[5]);
line("eq-undef-date", () => ds[5] === undefined);
line("eq-null-date", () => ds[5] === null);
line("loose-null-date", () => ds[5] == null);
line("neq-null-bigint", () => bs[5] != null);
line("truthy-date", () => (ds[5] ? "yes" : "no"));
line("truthy-bigint", () => (bs[5] ? "yes" : "no"));

// Using that answer as a receiver throws rather than reading the
// bare header block as a live value.
line("member-date", () => ds[5].getTime());
line("member-map", () => ms[5].size);
line("member-set", () => sets[5].size);

// Boxing, printing and serializing it.
line("box-any-date", () => {
  const a: any = ds[5];
  return a;
});
line("box-any-bigint", () => {
  const a: any = bs[5];
  return a;
});
line("arr-print-date", () => [ds[5]]);
line("arr-print-bigint", () => [bs[5]]);
line("json-date", () => JSON.stringify({ d: ds[5] }));
line("json-bigint", () => JSON.stringify({ b: bs[5] }));

const store = new Map<string, any>();
line("map-roundtrip", () => {
  store.set("k", ds[5]);
  return store.get("k");
});

function take(x: any) {
  return typeof x;
}
line("param-date", () => take(ds[5]));

// In-range reads keep answering the live value.
line("date-live", () => ds[0].getTime());
line("map-live-size", () => ms[0].size);
line("bigint-live", () => bs[0]);
line("zero-bigint-truthy", () => {
  const z: bigint = 0n;
  return z ? "yes" : "no";
});
