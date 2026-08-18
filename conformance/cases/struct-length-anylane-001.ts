// `.length` on an any-held struct cell (inline object literal's anon
// struct) — the length getter's Tag::Obj arm: own field probe first
// (member_get chunk-744 mirror), expando dict second, undefined on a
// genuine miss. Pre-fix the tag fell to the tail undefined, so an
// inline `{length: n, …}` array-like counted as 0.
function len(o: any): any {
  return o.length;
}
console.log(len({ length: 7 }));
console.log(len({ length: "str-len" }));
console.log(len({ notlength: 1 }));
console.log("[" + String.raw({ raw: { length: 2, 0: "A", 1: "B" } }, "-") + "]");
const boxed: any = { length: 3, 0: "x", 1: "y", 2: "z" };
console.log(boxed.length, boxed[2]);
console.log("done");
