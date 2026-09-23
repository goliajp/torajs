// A declared `string` slot receiving `null` through an `any` used to
// run §7.1.17 and store the four-character TEXT "null": `s === null`
// false, `typeof s` "string". NULL in a Str slot has denoted JS `null`
// since the undefined sentinel split the two (RFC 20260707 chunk 2);
// the any -> Str boundary kernels were the last place still spelling
// it as text.
const u: any = null;

let bare: string = u;
console.log("bare", bare, typeof bare, bare === null, bare === undefined);
console.log("bare ops", "x" + bare, `y${bare}`, JSON.stringify(bare), String(bare));

let ann: string | null = u;
console.log("ann", ann, typeof ann, ann === null);
console.log("ann ops", "x" + ann, `y${ann}`, JSON.stringify(ann));

function f(p: string | null) {
  console.log("param", p, typeof p, p === null, "x" + p);
}
f(u);

function g(): string | null {
  return u;
}
console.log("ret", g(), typeof g(), g() === null, "x" + g());

// A live string over the same boundaries is untouched.
const live: any = "ok";
let ls: string = live;
console.log("live", ls, typeof ls, ls === null, "x" + ls);

// The undefined member of the same family keeps its own spelling —
// the two are different values and now read differently everywhere.
const uu: any = undefined;
let us: string = uu;
console.log("undef", us, typeof us, us === undefined, "x" + us, JSON.stringify(us));

// Member access through either nullish throws, worded per shape.
try {
  console.log(bare.length);
} catch (e) {
  console.log("null.length throws TypeError:", e instanceof TypeError);
}
try {
  console.log(us.length);
} catch (e) {
  console.log("undef.length throws TypeError:", e instanceof TypeError);
}
