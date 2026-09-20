// §28.1.13 step 2 / §20.1.2.22 step 2 — "If proto is not an Object and
// proto is not null, throw a TypeError". tr asked `is_cell_imm`, which
// answers a narrower question: is this a heap cell. Strings, symbols and
// bigints are carried on the heap here and are primitives to the spec, so
// all three walked through a gate that meant "Object".

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label + ": " + String(f()));
  } catch (e) {
    console.log(label + ": " + (e as any).constructor.name);
  }
}

probe("reflect-symbol", () => Reflect.setPrototypeOf({}, Symbol("s") as any));
probe("reflect-bigint", () => Reflect.setPrototypeOf({}, 10n as any));
probe("reflect-longstring", () => Reflect.setPrototypeOf({}, "a long string that will not fit inline" as any));
probe("reflect-number", () => Reflect.setPrototypeOf({}, 1 as any));
probe("reflect-bool", () => Reflect.setPrototypeOf({}, true as any));
probe("reflect-undefined", () => Reflect.setPrototypeOf({}, undefined as any));

probe("object-symbol", () => Object.setPrototypeOf({}, Symbol("s") as any));
probe("object-bigint", () => Object.setPrototypeOf({}, 10n as any));

// Controls — null and a real object are the two the spec admits.
probe("reflect-null", () => Reflect.setPrototypeOf({}, null));
probe("reflect-object", () => Reflect.setPrototypeOf({}, { a: 1 }));
const o: any = {};
Reflect.setPrototypeOf(o, { tag: "proto" });
console.log("proto reached: " + String(o.tag));

// Control — the defineProperty gate reads the same predicate and is
// unchanged by naming it.
probe("defineProperty-on-symbol", () =>
  Object.defineProperty(Symbol("s") as any, "x", { value: 1 }));
probe("defineProperty-ok", () => {
  const t: any = {};
  Object.defineProperty(t, "x", { value: 1 });
  return t.x;
});
