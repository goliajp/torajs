// Generators only ever called directly, some calls handing them an
// `any` argument: each argument is bound as passed.

type P = { a: number };

function* gen(s: string) {
  yield s.length;
}
const g = function* (s: string, p: P) {
  yield s.length;
  yield p.a;
};
const h = gen;

const n: any = 5;
const str: any = "abcd";
const obj: any = { length: 6, a: 9 };
console.log(gen(n).next().value, gen(str).next().value, gen("ab").next().value);
console.log([...g(n, obj)].join(","), [...g("xyz", { a: 1 })].join(","), [...g(obj, n)].join(","));
console.log(h(n).next().value, h(str).next().value, h("ab").next().value);

// a generator only ever called with arguments that fit
function* plain(s: string) {
  yield s + "!";
}
for (const v of plain("ok")) console.log(v);
