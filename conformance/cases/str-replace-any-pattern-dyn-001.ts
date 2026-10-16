// an any-typed searchValue with no symbol-store evidence, under a
// replaceValue no typed kernel takes: the call decides RegExp-ness,
// IsCallable and ToString at run time
const lit: any = "b";
const rx: any = /b/g;
const fnAny: any = (m: string) => "<" + m + ">";
const fnTyped = (m: string) => "[" + m + "]";
console.log("abc".replace(lit, fnAny));
console.log("abcb".replaceAll(lit, fnAny));
console.log("abc".replace(lit, fnTyped));
console.log("abcb".replace(rx, "X"));
console.log("abcb".replace(rx, fnAny));
console.log("abcb".replace(rx, fnTyped));
console.log("abcb".replaceAll(rx, fnAny));
console.log("abcb".replace(rx, 1));
console.log("abcb".replace(rx, true));
console.log("abcb".replace(rx));
console.log("abcb".replace(rx, undefined));
console.log("abcb".replace(rx, null));
console.log("abcb".replace(lit, 1));
console.log("abcb".replace(lit));
console.log("abcb".replaceAll(lit, 2));
const out = "abcb".replace(rx, 3);
console.log(typeof out, out.length);
const nonG: any = /b/;
try {
  "abcb".replaceAll(nonG, fnAny);
  console.log("no throw");
} catch (e: any) {
  console.log(e instanceof TypeError);
}
const boom: any = {
  toString() {
    throw new Error("pat");
  },
};
try {
  "abcb".replace(boom, fnAny);
} catch (e: any) {
  console.log(e.message);
}
