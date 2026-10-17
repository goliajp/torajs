// The computed-key write reaches the RegExp through a binding typed
// `any` and through a dictionary, and a counting exec shows each typed
// String method calling it the number of times the spec says.
let calls = 0;
const re = /b/;
const holder: any = re;
const names = ["ex", "ec"];
holder[names.join("")] = function (s: string) {
  calls++;
  return null;
};
console.log("abc".search(re), calls);
console.log("abc".replace(re, "X"), calls);
console.log("abc".match(re), calls);
const d: { [k: string]: any } = { r: /c/ };
const inner: any = d["r"];
const k = "exec";
inner[k + ""] = () => null;
const cre: RegExp = d["r"];
console.log("abc".replace(cre, "Y"));
const nums: number[] = [1, 2];
nums[0] = 3;
console.log("abc".replace(/a/, "Z"), nums[0]);
