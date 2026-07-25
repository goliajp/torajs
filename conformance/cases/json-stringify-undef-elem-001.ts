// Rotation 208 — §25.5.2.4 step 8.a: an array element whose
// SerializeJSONProperty answers nothing (undefined, a callable, a
// Symbol) is written as `null`. The any-lane element walk reports
// that verdict as the undefined-Str sentinel, which the accumulator
// used to concatenate as the literal text `undefined`. Every static
// element lane already answered real text — a Str element goes
// through json_quote_str, whose nullish arm covers the sentinel —
// so only the `any` lane was leaking it.

console.log("A", JSON.stringify([undefined]));
console.log("B", JSON.stringify([undefined, 1]));
console.log("C", JSON.stringify([1, undefined, 2]));
console.log("D", JSON.stringify([true, undefined]));
console.log("E", JSON.stringify(["a", undefined]));

// Through a binding, and one level down.
const a = [undefined];
console.log("F", JSON.stringify(a));
console.log("G", JSON.stringify([[undefined]]));
console.log("H", JSON.stringify({ arr: [undefined] }));

// An `any` holding undefined reaches the element lane the same way.
const x: any = undefined;
console.log("I", JSON.stringify([x]));
console.log("J", JSON.stringify([1, x]));
function g(): undefined {
  return undefined;
}
console.log("K", JSON.stringify([1, g()]));

// The other two verdicts that serialize to nothing ride the same
// sentinel once an element sits in a mixed (`any`) array.
console.log("L", JSON.stringify([1, Symbol("x")]));
console.log("M", JSON.stringify(["a", Symbol("x")]));
const f: any = function () {};
console.log("N", JSON.stringify([1, f]));

// null keeps its own text — step 8.a only rewrites *nothing*.
console.log("O", JSON.stringify([null, undefined, 1]));
console.log("P", JSON.stringify([1, undefined, "b", true]));
