// Phase 1b: s.match / s.replace / s.replaceAll / s.split with regex.
// Capturing groups + $1 substitution are Phase 1c.

const m1 = "hello world".match(/world/);
console.log(m1.length);
console.log(m1[0]);

const m2 = "abc abc abc".match(/abc/g);
console.log(m2.length);
console.log(m2[0]);
console.log(m2[1]);
console.log(m2[2]);

const m3 = "x12y34z".match(/\d+/g);
console.log(m3.length);
console.log(m3[0]);
console.log(m3[1]);

const r1 = "hello world".replace(/world/, "earth");
console.log(r1);

const r2 = "Hello World".replace(/world/i, "earth");
console.log(r2);

const r3 = "abc abc abc".replaceAll(/abc/g, "X");
console.log(r3);

const r4 = "hello123world456".replace(/\d+/g, "_");
console.log(r4);

const sp1 = "a1b2c3d".split(/\d/);
console.log(sp1.length);
console.log(sp1[0]);
console.log(sp1[1]);
console.log(sp1[2]);
console.log(sp1[3]);

const sp2 = "abc".split(/x/);
console.log(sp2.length);
console.log(sp2[0]);

// Identifier-bound regex
const re = /\d+/g;
const m4 = "x12y34z".match(re);
console.log(m4.length);
console.log(m4[0]);
console.log(m4[1]);

// String-only paths still work (regression guard)
const r5 = "ab,cd,ef".split(",");
console.log(r5.length);
console.log(r5[0]);
console.log(r5[2]);

const r6 = "hello".replace("ll", "rr");
console.log(r6);
