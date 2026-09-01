// §13.2.8.4 GetTemplateObject — a site's template object is created
// once and cached; every evaluation hands the tag function that same
// (frozen) object, whichever lane the call takes. The boxed argv lane
// (a rest-taking closure) took the cached cell for a fresh call result
// and released it after the call, so from the second evaluation of a
// site on the tag read freed memory (the substitutions showed up where
// the strings belonged).
const viaRest = (strs: any, ...vals: any[]): string =>
  strs.join("|") + "#" + vals.length;
const viaPlain = (strs: any): string =>
  strs.join("|") + "/" + strs.raw[0].length + ":" + strs[0].length;
function viaDecl(strs: string[], ...vals: any[]): string {
  return strs.join("|") + "#" + vals.length;
}
for (let i = 0; i < 3; i++) {
  console.log(viaRest`a${i}b${i * 2}c`);
  console.log(viaPlain`x\ty${"z"}w`);
  console.log(viaDecl`p${i}q`);
}
const pre = "P";
const cap = (strs: any, ...vals: any[]): string =>
  pre + strs.join("|") + "#" + vals.join(",");
for (let i = 0; i < 3; i++) {
  console.log(cap`n${i}${i + 1}`);
}
let hits = 0;
const count = (strs: any, ...vals: any[]): number => {
  hits++;
  return strs.length + vals.length;
};
for (let i = 0; i < 4; i++) {
  count`a${i}b${i}c${i}`;
}
console.log(hits, count`z`);
