// §23.1.3.17 step 4 / §23.1.3.20 step 5 — `fromIndex` is ToIntegerOrInfinity'd,
// and when it is PRESENT there is no default left to take. The Arr<Any>
// lane folded every non-numeric spelling to 0 (indexOf) or the end
// (lastIndexOf), so `indexOf(1, "3E0")` answered a match at 1 and
// `lastIndexOf(x, undefined)` scanned the whole array instead of index 0.
const a: any[] = [0, 1, 2, 1];

console.log(a.indexOf(1, "3E0"), a.indexOf(1, "1"), a.indexOf(1, true));
console.log(a.indexOf(1, undefined), a.lastIndexOf(1, undefined));
console.log(a.indexOf(1, { valueOf() { return 2; } }));
console.log(a.lastIndexOf(1, "2"), a.lastIndexOf(1, 2), a.lastIndexOf(1));
console.log(a.indexOf(1, null), a.lastIndexOf(1, null));
console.log(a.indexOf(0, -100), a.lastIndexOf(2, -1));
console.log(a.indexOf(1, NaN), a.lastIndexOf(1, NaN));
console.log(a.indexOf(2, Infinity), a.lastIndexOf(0, -Infinity));

// §7.1.1 ToPrimitive — a valueOf/toString pair that both answer objects
// is the TypeError the coercion owes, raised from this position.
try {
  a.indexOf(1, { valueOf() { return {}; }, toString() { return {}; } } as any);
  console.log("NO THROW");
} catch (e: any) { console.log(e.constructor.name); }

// The typed-element lane has read it this way since rotation 629 —
// the two must agree.
const t: number[] = [0, 1, 2, 1];
console.log(t.indexOf(1, "3E0" as any), t.lastIndexOf(1, undefined as any));
