// S303 — Array.{sort,toSorted}(undef, ...trailing) per ES §23.1.3.{30,33}
// trailing-arg ignore. check.rs S276's args.len() >= 2 branch gated cmp to
// Function | Any — undef cmp + trailing fell through to generic arity
// rejection ("expected 1 argument(s), got 2"). Widen to also accept
// Undefined (ssa_lower S276 mirror already routes undef to default-compare
// + drops trailing per the prior eval-and-drop loop).

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

const xs = [3, 1, 4, 1, 5, 9, 2, 6];

// sort(undef, trailing) — default lex comparator
calls = 0;
const r1 = xs.slice().sort(undefined, step("s1") as any);
console.log("sort-undef-1 calls=", calls, "first=", r1[0], "last=", r1[r1.length - 1]);

// sort(undef, t1, t2)
calls = 0;
const r2 = xs.slice().sort(undefined, step("s2a") as any, step("s2b") as any);
console.log("sort-undef-2 calls=", calls, "first=", r2[0], "last=", r2[r2.length - 1]);

// toSorted(undef, trailing) — fresh-array variant
calls = 0;
const r3 = xs.toSorted(undefined, step("ts1") as any);
console.log("toSorted-undef-1 calls=", calls, "first=", r3[0], "last=", r3[r3.length - 1]);

calls = 0;
const r4 = xs.toSorted(undefined, step("ts2a") as any, step("ts2b") as any, step("ts2c") as any);
console.log("toSorted-undef-3 calls=", calls, "first=", r4[0], "last=", r4[r4.length - 1]);

// sort(cmp, trailing) — existing path stays correct
calls = 0;
const r5 = xs.slice().sort((a: number, b: number) => b - a, step("c1") as any);
console.log("sort-cmp-1 calls=", calls, "first=", r5[0], "last=", r5[r5.length - 1]);
