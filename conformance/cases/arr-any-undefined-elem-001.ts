// RFC 20260721 刀 12 G15 — an Undefined-typed variable element in a
// heterogeneous array literal keeps its undefined identity (the
// checker walks EVERY element even after the Any widening verdict,
// so the pack reads the recorded type instead of collapsing the
// ConstPtrNull to null).
let u;
let a = [1, null, u];
console.log(a[2], typeof a[2], a[2] === undefined, a.indexOf(undefined));
let b = [1, u];
console.log(b[1], typeof b[1]);
let c = [null, u];
console.log(c[1], typeof c[1], c[0], typeof c[0]);
let d = new Array(true, null, u);
console.log(d[2], typeof d[2], d.lastIndexOf(undefined));
