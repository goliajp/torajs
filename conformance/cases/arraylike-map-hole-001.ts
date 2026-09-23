// §23.1.3.20 step 5.c — `map` sets the result's index only when the
// source HAS that index, so an absent one leaves a hole in the
// product. The array-like walker filled it with tag 0, which is
// `null`: the slot read back as null, `typeof` said "object" and
// `k in result` said true, where all three must report absence.
const o: any = { 0: 1, 2: 3, length: 4 };
const r: any = (Array.prototype.map as any).call(o, function (v: any) { return v * 2; });
console.log(r.length, r[1], typeof r[1], 1 in r, 0 in r);
console.log(JSON.stringify(r));
console.log(r.map(function (v: any) { return v; }).length);

// forEach / filter already skipped absent indexes — they must not move.
let visited = 0;
(Array.prototype.forEach as any).call(o, function () { visited++; });
console.log(visited);
const f: any = (Array.prototype.filter as any).call(o, function () { return true; });
console.log(f.length, JSON.stringify(f));

// A real array with holes takes the Arr kernel's own map arm, which
// has always emitted the hole — the two must agree.
const arr: any[] = [1, , 3];
const ar: any = arr.map(function (v: any) { return v * 2; });
console.log(ar.length, ar[1], typeof ar[1], 1 in ar, JSON.stringify(ar));
