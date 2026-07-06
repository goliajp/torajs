// RC-4 F1a (RFC 20260706-test262-bug-corpus) — re.exec(s) /
// s.match(re) retype to Nullable<Array<Str>> per spec §22.2.6.2 /
// §22.1.3.13 (null on miss). V3-18 narrowing (`if (m !== null)`)
// yields the bare Array<Str> at zero cost; un-narrowed
// member/index consumption decays to the array (null case gets a
// runtime TypeError guard).

// Narrowed path — the bench-friendly shape.
let m = /(\d+)/.exec("abc 123 xyz");
if (m !== null) {
  console.log(m[0], m[1], m.length);
}

// Miss really is null now.
let miss = /zzz/.exec("abc");
console.log(miss === null);

// match mirrors exec.
let mm = "hello world".match(/o (w)/);
if (mm !== null) {
  console.log(mm[0], mm[1]);
}

// Un-narrowed decay consumption on a hit: member + expando
// index/input + element indexing all see the bare array.
let d = "hello".match(/ell/);
console.log(d.length, d[0], d.index, d.input);

// Truthy-narrow wedge form.
let t = /w(or)ld/.exec("hello world");
if (t) {
  console.log(t[1]);
}
