// L3b loop-push-empty-array regression lock (rotation 173 record,
// unreproducible at ace1bb30) — push of non-Ident args inside loop
// bodies must not silently no-op.
let c = ["a", "b"];
let out = [];
for (let i = 0; i < c.length; i++) {
  out.push(c[i]);
}
console.log(out.length);
let out2 = [];
for (let i = 0; i < c.length; i++) {
  out2.push(String(c[i]));
}
console.log(out2.length);
let out3 = [];
for (let i = 0; i < c.length; i++) {
  let s = c[i];
  out3.push(s);
}
console.log(out3.length);
let out4 = [];
out4.push(c[0]);
console.log(out4.length);
let c2 = ["a", "b", "c"];
let r1 = [];
for (let i = 0; i < c2.length; i++) {
  if (c2[i] != "b") {
    r1.push(c2[i]);
  }
}
console.log(r1.length);
let r2 = [];
for (const x of c2) {
  r2.push(x);
}
console.log(r2.length);
let r3 = [];
let j = 0;
while (j < c2.length) {
  r3.push(c2[j]);
  j++;
}
console.log(r3.length);
let r4 = [];
c2.forEach((x) => { r4.push(x); });
console.log(r4.length);
