// await-dictionary Promise.allKeyed / allSettledKeyed read as
// VALUES (rotation 452) — bun 1.3.14 does not implement the
// proposal, so the oracle lives in the sibling .expected file
// (spec semantics: `.length` 1 / `.name` from the function
// definition, %Function.prototype% behind the cell, step-1
// IsConstructor TypeError on `.call(eval)`, and a
// receiver-honoring `.call(Promise, obj)` running the real keyed
// kernel).
const ak: any = (Promise as any).allKeyed;
const ask: any = (Promise as any).allSettledKeyed;
console.log(typeof ak, ak.length, ak.name);
console.log(typeof ask, ask.length, ask.name);
console.log(Object.getPrototypeOf(ak) === Function.prototype);
try {
  ak.call(eval, { a: 1 });
  console.log("no throw");
} catch (e: any) {
  console.log("TypeError", e instanceof TypeError);
}
ak.call(Promise, { a: 1, b: Promise.resolve(2) }).then((r: any) => {
  console.log("keyed", r.a, r.b);
});
