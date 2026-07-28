// [Symbol.iterator] read off native iterable tags reifies the
// spec-aliased prototype method (§23.1.3.40 Array = values,
// §24.1.3.12 Map = entries, §24.2.3.11 Set = values); same interned
// cell as the named read, so the alias identity holds and .call
// re-binds the receiver
const a: any = [1, 2];
console.log(typeof a[Symbol.iterator]);
const f: any = a[Symbol.iterator];
console.log(f === a.values);
const it: any = f.call(a);
console.log(it.next().value);
console.log(it.next().value);
console.log(it.next().done);

const m: any = new Map();
m.set("k", 9);
console.log(typeof m[Symbol.iterator]);
console.log(m[Symbol.iterator] === m.entries);
const mi: any = m[Symbol.iterator].call(m);
console.log(mi.next().value);

const s: any = new Set([7]);
console.log(s[Symbol.iterator] === s.values);
// §24.2.4.8 keys IS values; @@iterator joins the same identity
console.log(s[Symbol.iterator] === s.keys);
const si: any = s[Symbol.iterator].call(s);
console.log(si.next().value);

// a plain object stays undefined (no native iterator to reify)
const o: any = { x: 1 };
console.log(typeof o[Symbol.iterator]);
