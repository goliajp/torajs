// §23.1.3 — forEach / map / filter / some / every / reduce /
// reduceRight gate each visit on HasProperty, so a hole is skipped and
// the callback never sees it. find / findLast / findIndex /
// findLastIndex are NOT in that list: §23.1.3.9 Gets every index.
function fresh(): any[] { let a: any[] = [1, 2, 3]; delete a[1]; return a; }

let hits: any[] = [];
fresh().forEach(function (v: any, i: number) { hits.push(i); });
console.log("forEach  :", hits.join(","));
console.log("filter   :", JSON.stringify(fresh().filter(function (v: any) { return true; })));
console.log("some     :", fresh().some(function (v: any) { return v === undefined; }));
console.log("every    :", fresh().every(function (v: any) { return v !== undefined; }));
let seen: any[] = [];
console.log("reduce   :", fresh().reduce(function (a: any, v: any, i: number) { seen.push(i); return a; }, 0), seen.join(","));
let seenR: any[] = [];
console.log("reduceR  :", fresh().reduceRight(function (a: any, v: any, i: number) { seenR.push(i); return a; }, 0), seenR.join(","));
console.log("find     :", fresh().find(function (v: any) { return v === undefined; }));
console.log("findIndex:", fresh().findIndex(function (v: any) { return v === undefined; }));

// A boxed-element product carries the skipped index through as a hole
// of its own.
let m = fresh().map(function (v: any) { return v; });
console.log("map      :", JSON.stringify(m), m.length, 1 in m);

// An elision hole is the same absence.
let e = [1, , 3];
let hit: any[] = [];
e.forEach(function (v: any, i: number) { hit.push(i); });
console.log("elision  :", hit.join(","), e.some(function (v: any) { return v === undefined; }));

// A dense array visits every index and keeps its typed lane.
let d = [1, 2, 3];
let s = 0;
d.forEach(function (v: number) { s = s + v; });
console.log("dense    :", s, JSON.stringify(d.map(function (v: number) { return v * 2; })),
  d.filter(function (v: number) { return v > 1; }).join(","),
  d.some(function (v: number) { return v === 2; }),
  d.every(function (v: number) { return v > 0; }),
  d.reduce(function (a: number, v: number) { return a + v; }, 0));

// The gate is emitted only for a boxed-element source: an unboxed slot
// has no value that means absent, so a typed array cannot hold an
// interior hole in the first place. A `number[]` therefore emits the
// same loop it did before this rule existed — measured byte-identical.
let big: number[] = [];
for (let i: number = 0; i < 4; i = i + 1) { big.push(i); }
let acc: number = 0;
big.forEach(function (v: number) { acc = acc + v; });
console.log("typed    :", acc, big.reduce(function (a: number, v: number) { return a + v; }, 0),
  big.some(function (v: number) { return v === 2; }), big.every(function (v: number) { return v >= 0; }));
