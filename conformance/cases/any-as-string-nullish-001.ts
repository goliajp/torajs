// `x as string` on an `any` holding null / undefined is no ToString:
// the value stays null / undefined (TS `as` has no run-time effect)
const nul: any = null;
const un: any = undefined;
const s1 = nul as string;
console.log(s1 === null, typeof s1, String(s1));
const s2 = un as string;
console.log(s2 === undefined, typeof s2, s2 + "!");
const word: any = "abc";
const s3 = word as string;
console.log(s3.length, typeof s3, s3.toUpperCase());
function pick(v: any): string {
  const s = v as string;
  return s === undefined ? "none" : s === null ? "null" : s;
}
console.log(pick(undefined), pick(null), pick("x"));
