// A computed key whose type an `as` names a harmless literal while the
// value is built at run time (ECMA-262 §13.15.2): a type does not tell
// which property a key names, so the `then` stored under it is found by
// the await (§27.2.1.3.2 steps 9-13).

const k = ["n", "e", "h", "t"].reverse().join("") as "a";
const o = { a: 1 };
const oa: any = o;
oa[k] = (r: (v: number) => void) => r(42);

async function f() {
  const v = await o;
  console.log(typeof v, v);
}

f();
