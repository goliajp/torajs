// §13.2.4.1 runs every spread through GetIterator, and
// GetMethod(undefined, @@iterator) is a TypeError — the same answer
// `for (const v of r)` already gave. The typed spread lanes read the
// source's backing store instead of iterating it, so a pointer slot
// holding the generic immortal cell was walked as a live array:
// `[...xs.find(p)]` on a miss was a SIGSEGV.

const xs: number[][] = [];
const r = xs.find((z: number[]) => z.length > 99);

try {
  console.log("array-spread", "ok", [...r].length);
} catch (e) {
  console.log("array-spread", "threw", (e as Error).constructor.name);
}

try {
  console.log("mixed-spread", "ok", [0, ...r, 9].length);
} catch (e) {
  console.log("mixed-spread", "threw", (e as Error).constructor.name);
}

function g(...a: number[]): number {
  return a.length;
}
try {
  console.log("call-spread", "ok", g(...r));
} catch (e) {
  console.log("call-spread", "threw", (e as Error).constructor.name);
}
try {
  console.log("call-spread-head", "ok", g(1, ...r));
} catch (e) {
  console.log("call-spread-head", "threw", (e as Error).constructor.name);
}

// `apply_rest_args` spells the rest-parameter call above as
// `Array.from(r)`, so that surface is the one that actually ran —
// and it is reachable directly too (§23.1.2.1 step 4, GetMethod).
try {
  console.log("array-from", "ok", Array.from(r).length);
} catch (e) {
  console.log("array-from", "threw", (e as Error).constructor.name);
}
try {
  console.log("array-from-map", "ok", Array.from(r, (v: number) => v + 1).length);
} catch (e) {
  console.log("array-from-map", "threw", (e as Error).constructor.name);
}

// --- for-of on the same slot already answered, and still does
try {
  for (const v of r) {
    console.log("unreached", v);
  }
  console.log("for-of", "ok");
} catch (e) {
  console.log("for-of", "threw", (e as Error).constructor.name);
}

// --- the annotated spelling of the same slot
const a: number[] | undefined = undefined;
try {
  console.log("annotated", "ok", [...a].length);
} catch (e) {
  console.log("annotated", "threw", (e as Error).constructor.name);
}

// --- live sources are untouched, in every family the lanes know
const live = [[1, 2], [3]];
const hit = live.find((z: number[]) => z.length > 1);
console.log([...hit].join("-"), [0, ...hit, 9].join("-"), g(...hit));
console.log([..."ab"].join("-"), [...new Set([1, 1, 2])].join("-"));
console.log(Array.from(hit).join("-"), Array.from("ab").join("-"));
console.log(Array.from(hit, (v: number) => v * 2).join("-"));
console.log([...[1, 2], ...[3]].join("-"));
const m = new Map<string, number>([["a", 1]]);
console.log([...m.keys()].join("-"));

// --- and the source still evaluates exactly once
let visits = 0;
console.log([
  ...live.find(() => {
    visits = visits + 1;
    return true;
  }),
].join("-"));
console.log("visits", visits);
