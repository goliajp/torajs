// Registered defects that a batch re-verification found are already
// fixed. Each was closed by some change that never pinned it, so each
// was one edit away from coming back silently.
//
// The re-verification is the point: of eleven registrations checked
// this rotation, TEN answered correctly. A stale registration is not
// free — it sends the next rotation to hunt something that no longer
// exists, and this batch cost one probe run to clear.
//
// Sibling of `registered-then-fixed-guards-001`, which did the same
// for the previous batch.

// r689 (8) — an array LITERAL as a member-call receiver was recorded
// as having no lowering path at all ("unsupported member call shape").
console.log("push", [1, 2].push(3));
console.log("join", [1, 2].join("-"));
console.log("map", [1, 2].map((x) => x * 2).join(","));
console.log("pop", [1, 2].pop());
console.log("concat", [1, 2].concat([3]).join(","));
console.log("sort", [3, 1, 2].sort((a, b) => a - b).join(","));
console.log("index", [1, 2][0]);
console.log("objlit-call", ({ m() { return 5; } }).m());

// r689 (9) — the extra arguments of a callable coercion were recorded
// as not evaluated. §13.3.6.1 evaluates every argument; only the
// binding stops at the declared formals.
let sideA = 0;
function bumpA(): number { sideA = 1; return 0; }
console.log("num-extra", Number(1, bumpA()), sideA);
let sideB = 0;
function bumpB(): number { sideB = 1; return 0; }
console.log("str-extra", String("x", bumpB()), sideB);

// r689 (7) — `delete o[12n]` was refused by the checker; §13.5.1.2
// goes through ToPropertyKey, which takes a BigInt.
const bagA: any = {};
bagA[12n] = 1;
console.log("delete-bigint", delete bagA[12n]);

// r689 (5) — a regex-shaped `any` needle through replace was recorded
// as failing at link time ("arr method family stripped").
const needle: any = [12];
console.log("replace-any", "x12y".replace(needle, "Z"));

// r688 (3) — a member WRITE on a primitive receiver: strict mode
// makes it a TypeError, which is a runtime throw and not a refusal.
const s: string | undefined = "ab";
try {
  (s as any).foo = 1;
  console.log("prim-write", "no throw");
} catch (e) { console.log("prim-write", (e as Error).constructor.name); }

// r689 (11) — a function with its own `toString` must answer it, not
// its source text.
function plain(): number { return 1; }
(plain as any).toString = () => "CUSTOM";
console.log("fn-tostring", String(plain));

// §20.4.2.2 step 1 is ToString(key), so a non-string key is legal.
console.log("symbol-for-number", String(Symbol.for(12)));

// an `any` bag reached through both spellings
const bagB: any = {};
bagB["p"] = 3;
console.log("any-bag", bagB.p);
