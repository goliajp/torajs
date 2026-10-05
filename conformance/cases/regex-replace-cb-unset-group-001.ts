// a replace callback naming a capture group that some match leaves
// unset: §22.2.6.11 step 14.b binds `undefined` to that parameter
const s = "ab-cd";
const out = s.replace(/(a)|(c)/g, (m: string, g1: string, g2: string) => {
  return "[" + (g2 === undefined) + "," + typeof g2 + "," + typeof g1 + "," + String(g2) + "]";
});
console.log(out);
console.log("xay".replace(/x(b)?a/, (m: string, b: string) => typeof b + ":" + (b === undefined)));
console.log("xay".replace(/x(b)*a/, (m: string, b: string) => typeof b));
console.log("xay".replace(/x(?!(b))a/, (m: string, b: string) => typeof b));
console.log("xay".replace(/x(?:(b)|a)/, (m: string, b: string) => typeof b));
// every group set on every match keeps the typed lane
console.log("k1=v1;k2=v2".replace(/(\w+)=(\w+)/g, (m: string, k: string, v: string) => v + "=" + k));
console.log("aaa".replace(/(a)+/, (m: string, g: string) => typeof g + g));
// an unset group past the declared parameters is not read
console.log("xa".replace(/(x)(b)?a/, (m: string, x: string) => x.toUpperCase()));
console.log("q1".replace(/(q)(\d)|z/, (m: string, q: string, d: string, off: number, str: string) => typeof q + typeof d + off + str));
