// flat / flatMap skip holes (§23.1.3.13.1 FlattenIntoArray asks
// HasProperty per index): a number[] or string[] with holes at the leaf
// layer, an outer number[][] whose hole is a NULL slot (was a SIGSEGV),
// an any[] source of flatMap.
function mk(): number[] {
  let a = [1.5, 2.5];
  a[4] = 3.5;
  return a;
}
console.log(mk().flat(), mk().flat(0), mk().flat(2));
console.log(mk().flatMap((v) => [v, v]), mk().flatMap((v) => v * 2), mk().flatMap((v, i) => [i]));
const s = ["a"];
s[2] = "c";
console.log(s.flat(), s.flatMap((x) => [x]));
const n = [[1], [2, 3]];
n[3] = [4];
console.log(n.flat(), n.flatMap((x) => x), n.length);
const an: any[] = [1, , [2, , 3]];
console.log(an.flat(), an.flatMap((x: any) => [x]));
console.log([1, 2].flat(), [[1.5], [2.5]].flat(), [1, 2].flatMap((v) => [v]));
