// `any` functions entering a function array through the copying methods
// and literals: `with`, `toSpliced`, `concat`, `Array.of` and a spread of
// an `any[]` into an `F[]` literal. A call through the copy answers what
// the stored function returns, whatever its own arity and return.
type F = (x: number) => number;
const tag: any = (x: any) => "t" + x;
const q: any = (x: any) => x / 4;
const two: any = (a: any, b: any) => (b === undefined ? "u" + a : a + b);
const hs: F[] = [(x: number) => x + 1, (x: number) => x * 2];

const w = hs.with(0, tag);
console.log(w[0](1), w[1](1), hs[0](1));
const w2 = hs.with(1, q);
console.log(w2[1](10), w2[0](10), hs[1](10));
const w3: F[] = hs.with(-1, two);
console.log(w3[1](2), w3.at(-1)!(3));

const s = hs.toSpliced(1, 0, two);
console.log(s[1](3), s[2](3), s.length);
const c = hs.concat([q]);
console.log(c[2](3), c.at(-1)!(5));
const o: F[] = Array.of(q, two);
console.log(o[0](4), o[1](4));

const anys: any[] = [q, two];
const sp: F[] = [...anys, ...hs];
console.log(sp[0](5), sp[1](5), sp[2](5));
const one: any[] = [tag];
const sp2: F[] = [...one];
console.log(sp2[0](5));
const m: F[] = hs.concat(anys);
console.log(m[2](7), m[3](7));
