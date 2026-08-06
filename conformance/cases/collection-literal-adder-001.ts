// torajs follows the spec here; bun does not, so this case carries a
// `.expected`. §24.1.1.1 step 7.a reads the adder off the target
// ONCE — before any entry is added, and whether or not the source is
// a literal the implementation could see through. bun performs step
// 7.c (the IsCallable throw) only when it cannot see the source
// statically: `new Map([[1, 1]])` with a non-callable `set` adds the
// entries anyway, where V8 and the spec throw. The `.expected` here
// is node's output, verbatim.
//
// bun does consult a *callable* patch on a literal source, so the
// rest of this case agrees three ways.

const nativeMapSet = (Map.prototype as any).set;
const nativeSetAdd = (Set.prototype as any).add;

// --- the adder is read even when nothing will be added: step 7 runs
//     before the walk, so an empty source throws too ---
(Map.prototype as any).set = null;
try {
  new Map([]);
  console.log("empty literal: no throw");
} catch (e: any) {
  console.log("empty literal:", e instanceof TypeError);
}

// --- a populated literal: same throw, and nothing was added ---
try {
  new Map([
    [1, 1],
    [2, 2],
  ]);
  console.log("literal: no throw");
} catch (e: any) {
  console.log("literal:", e instanceof TypeError);
}

// --- a callable patch takes every entry, and the native store never
//     runs (the collection stays empty) ---
let seen: string = "";
(Map.prototype as any).set = function (k: any, v: any) {
  seen = seen + k + "=" + v + ";";
  return null;
};
const patched: any = new Map([
  ["a", 1],
  ["b", 2],
]);
console.log("patched map", seen, patched.size);

let added: string = "";
(Set.prototype as any).add = function (v: any) {
  added = added + v + ";";
  return null;
};
const patchedSet: any = new Set([1, 2, 3]);
console.log("patched set", added, patchedSet.size);

// --- an adder that throws stops the literal where it threw ---
let calls = 0;
(Set.prototype as any).add = function () {
  calls = calls + 1;
  if (calls === 2) {
    throw new Error("stop");
  }
  return null;
};
try {
  new Set([1, 2, 3]);
  console.log("throwing adder: no throw");
} catch (e: any) {
  console.log("throwing adder stopped at", calls);
}

// --- restored, the literal lane fills natively again ---
(Map.prototype as any).set = nativeMapSet;
(Set.prototype as any).add = nativeSetAdd;
const plainMap = new Map([
  [1, "a"],
  [2, "b"],
]);
console.log("plain map", plainMap.size, plainMap.get(1), plainMap.get(2));
const plainSet = new Set([1, 2, 2, 3]);
console.log("plain set", plainSet.size, plainSet.has(2), plainSet.has(9));
