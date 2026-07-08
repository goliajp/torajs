// RFC 20260708-typed-arr-oob-read chunk 3 — ToString + tag/value
// consumers of the F64 undefined sentinel (number[] OOB read).
const a: number[] = [1.5, 2.5];

// template literal (parser desugars to concat chain)
console.log(`t=${a[9]}`);
console.log(`t=${a[0]}`);

// concat — sentinel on either side
console.log("l=" + a[9]);
console.log(a[9] + "=r");
console.log("l=" + a[1]);

// arithmetic result must stay NaN (payload propagates, static gate excludes)
console.log("n=" + (a[9] + 1));

// multi-arg print
console.log("m:", a[9], a[0]);

// let alias rides the infection set
let v = a[9];
console.log(`alias=${v}`);
console.log("alias:", v);

// Map.set value — round-trips a real undefined
const m = new Map<string, number>();
m.set("k", a[9]);
m.set("k2", a[0]);
console.log(m.get("k") === undefined);
console.log(m.get("k"));
console.log(m.get("k2"));

// Map key
const mk = new Map<number, string>();
mk.set(a[9], "u");
console.log(mk.get(a[9]));
console.log(mk.size);

// Set.add
const s = new Set<number>();
s.add(a[9]);
console.log(s.has(a[9]));
console.log(s.size);
