// A computed key typed as a literal by a parameter, handed a key built
// at run time through an `any` (ECMA-262 §13.15.2, §27.2.1.3.2).

function put(o: any, k: "a", f: unknown) {
  o[k] = f;
}

const s = ["th", "en"].join("");
const o = { a: 1 };
put(o, s as any, (r: (v: number) => void) => r(42));

async function f() {
  const v = await o;
  console.log(typeof v, v);
}

f();
