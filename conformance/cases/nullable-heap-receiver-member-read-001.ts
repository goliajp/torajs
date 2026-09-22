// A `T | undefined` receiver reaches the member lane for every
// pointer-shaped T, not just arrays.
//
// §13.3.4 puts no static condition on the base of a property access:
// the Reference Record is built unconditionally and the TypeError is
// raised later, by §6.2.5.5 GetValue -> §7.1.18 ToObject. So refusing
// at compile time refused programs the spec fully defines. tr stripped
// the wrapper only for `Nullable<Array>`; `o.a`, `m.size`, `s.size`
// and `c.v` were all refused, on programs bun runs and prints.
//
// Which T may strip is `nullable_ptr_shaped` — the same question three
// other sites already ask. The two scalars stay out (their nullable
// rides the NaN-boxed any lane) and so does String, whose call-side
// guard does not exist yet.
//
// Written out directly, never through a helper: a closure or a
// parameter does not carry the receiver's annotation (r687).

class C {
  v: number = 3;
  m(): number {
    return 4;
  }
}

function maybeObj(): { a: number } | undefined {
  return Math.random() < 2 ? { a: 1 } : undefined;
}
function noObj(): { a: number } | undefined {
  return undefined;
}
function maybeMap(): Map<string, number> | undefined {
  return Math.random() < 2 ? new Map() : undefined;
}
function maybeSet(): Set<number> | undefined {
  return Math.random() < 2 ? new Set() : undefined;
}
function maybeCls(): C | undefined {
  return Math.random() < 2 ? new C() : undefined;
}

function main(): void {
  const o: { a: number } | undefined = maybeObj();
  console.log(o.a);

  const m: Map<string, number> | undefined = maybeMap();
  console.log(m.size);
  m.set("a", 1);
  console.log(m.get("a"));

  const s: Set<number> | undefined = maybeSet();
  console.log(s.size, s.has(1));

  const c: C | undefined = maybeCls();
  console.log(c.v);

  // Same slots, holding undefined: the guard already in the lowering
  // must raise the TypeError §7.1.18 asks for — not a wrong value.
  const gone: { a: number } | undefined = noObj();
  try {
    console.log(gone.a);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
}

main();
