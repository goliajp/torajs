// §13.4.4.1 / §13.4.5.1 — an update expression whose ToNumeric throws
// (a user valueOf) completes abruptly and stores nothing.
let o: any = {
  valueOf() {
    throw new Error("valueOf");
  },
};
for (const run of [() => o++, () => ++o, () => o--, () => --o]) {
  try {
    run();
    console.log("no throw");
  } catch (e: any) {
    console.log("caught", e.message, typeof o);
  }
}
const holder: any = { v: o };
try {
  holder.v++;
} catch (e: any) {
  console.log("member", e.message, typeof holder.v);
}
const key = "v";
try {
  holder[key]--;
} catch (e: any) {
  console.log("keyed", e.message, typeof holder[key]);
}
let f: any = function () {};
f++;
console.log(typeof f, f);
let s: any = "41";
console.log(s++, s);
