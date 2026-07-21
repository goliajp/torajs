// RFC 20260721-typed-grow-on-write — a typed block index write past
// the end grows-as-holes (gap indexes are not own properties).
// i64 lane: plain append, then gap grow
let a = [1, 2, 3];
a[3] = 4;
console.log(a.length, a[3]);
a[8] = 9;
console.log(a.length, a[8]);
console.log(5 in a, 8 in a);
console.log(a.indexOf(9));
// f64 lane
let f = [1.5, 2.5];
f[4] = 9.5;
console.log(f.length, f[4]);
// str lane (heap cells — the slot takes its own stake)
let s = ["x"];
s[2] = "z";
console.log(s.length, s[2], 1 in s, s.indexOf("z"));
// bool lane
let b = [true];
b[2] = false;
console.log(b.length, b[2]);
// deque-shifted receiver: head compacts before the grow
let d = [10, 20, 30];
d.shift();
d[5] = 60;
console.log(d.length, d[0], d[5], 3 in d);
// any[] view over a typed block: past-end write kind-coerces + grows
let v: any[] = [1, 2];
v[4] = 7;
console.log(v.length, v[4], 3 in v, 4 in v);
