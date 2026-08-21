// §22.1.2.4 String.raw as a VALUE. The call forms already worked --
// direct and tagged-template both lower per-shape -- but reading the
// function itself was a compile error, so `String.raw.name`,
// `String.raw.length`, its property descriptors, and a detached call
// through a binding had no answers at all.

console.log(typeof String.raw, String.raw.name, String.raw.length);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(String.raw, "length")));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(String.raw, "name")));

// §22.1.2.4 is not a constructor.
try {
  new (String.raw as any)();
  console.log("constructed");
} catch (e: any) {
  console.log("threw", e instanceof TypeError);
}

// Detached: the cell's dispatch reads slot 0 as the template and the
// tail as substitutions.
const r = String.raw;
console.log(r({ raw: ["a", "b"] } as any, 1));
console.log(r({ raw: ["x", "y", "z"] } as any, 1, 2));
console.log(r({ raw: ["only"] } as any));
console.log(r({ raw: ["p", "q"] } as any, 1, 2, 3));
console.log(r.call(null, { raw: ["c", "d"] } as any, 9));
console.log(r.apply(null, [{ raw: ["e", "f"] } as any, 8]));

// Step 2's ToObject rejects a nullish template.
try {
  (r as any)(null);
} catch (e: any) {
  console.log("nullish threw", e instanceof TypeError);
}

// The call forms are unchanged.
console.log(String.raw`p${1}q${2}r`);
console.log(String.raw({ raw: ["a", "b"] } as any, 1));
