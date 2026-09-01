// 551-03 — a tag function written as a top-level `const` arrow. The
// template object reaches the call as a nanbox Any; against a typed
// `strs: string[]` param the any-widen lane clones the LIFTED body
// with that slot widened and retargets the call to the clone, entered
// with the original cell's env. Every shape below is one the clone
// has to serve: a rest tail (boxed dual entry), no rest (env-first
// native call), captures, a typed caller sharing the original, the
// `.raw` face, and calls from a block / a loop / a callback / a
// callback nested in another closure.
const pre = "P";
let calls = 0;

const tag = (strs: string[], ...vals: any[]): string => {
  calls++;
  return pre + strs.join("|") + "#" + vals.length;
};
const plain = (strs: string[]): string => pre + strs.join("+");
const raw = (strs: string[], ...vals: any[]): string =>
  strs.raw[0] + "/" + strs[1] + "/" + vals.join(",");

const x = 1;
console.log(tag`a${x}b`);
console.log(tag`c`);
console.log(plain`ab`);
console.log(raw`a\n${1}b${2}`);

// the original stays live for its typed callers
console.log(tag(["x", "y"], 1, 2));
console.log(plain(["q", "r"]));

{
  console.log(tag`x${1}y`);
}
for (let i = 0; i < 2; i++) console.log(tag`i${i}`);
console.log([1, 2].map((n) => tag`n${n}`).join(";"));
const outer = (k: number): string =>
  [1, 2].map((n) => tag`n${n}${k}`).join(";");
console.log(outer(7));
console.log([1, 2].map((n) => plain`p${n}` + n).join(";"));
console.log(calls);
