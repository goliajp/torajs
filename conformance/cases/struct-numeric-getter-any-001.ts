// Numeric-key accessors on an un-annotated object literal (anon
// struct lane) read through any: `a[0]` must resolve the `__getter_0`
// layout slot exactly like the named-getter member lane (`o[0]` ≡
// `o["0"]`, ES §7.1.19), and a set-only numeric accessor reads as
// undefined (§10.1.8).
let s = { length: 2, get 0() { return "g0"; }, get 1() { return 41 + 1; } };
let a: any = s;
console.log(a.length, a[0], a[1]);
let so = { set 0(v: any) { console.log("sink", v); } };
let ao: any = so;
console.log(ao[0]);
console.log(a[9]);
