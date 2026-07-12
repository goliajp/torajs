// SameValueZero key canonicalization (§7.2.10) across Map/Set:
// -0 stores as +0; integral-f64 and i64 keys unify; BigInt keys
// compare by value, not allocation identity.

// -0 → +0 normalization on add (§24.2.4.1)
const s = new Set();
s.add(-0);
console.log(s.has(0), s.has(-0), [...s][0], String([...s][0]), [...s][0] === 0);
s.delete(0);
console.log(s.size);

// Map key -0 (§24.1.3.9)
const m = new Map<any, any>([[-0, "a"]]);
console.log(m.has(0), m.get(0));

// number vs bigint with the same mathematical value stay distinct,
// but each is findable through its own type
const number = 9007199254740991;
const bigint = 9007199254740991n;
const sv = new Set([number, bigint] as any[]);
console.log(sv.size, sv.has(number), sv.has(bigint as any));
sv.delete(number);
console.log(sv.size, sv.has(number), sv.has(bigint as any));

// BigInt keys hit by value across separate literals
const sb = new Set([1n]);
console.log(sb.has(1n), sb.has(2n));
const mb = new Map<any, any>([[10n, "ten"]]);
console.log(mb.get(10n), mb.has(-10n));

// boxed integral f64 (via any[] element) unifies with i64 lookups
const arr: any[] = [7];
const m3 = new Map<any, any>([[arr[0], "y"]]);
console.log(m3.has(7), m3.get(7));
