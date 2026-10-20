// The only `then` is stored under a computed key the program builds
// at run time (ECMA-262 §13.15.2): the key's type, a string, does not
// tell it from any other name.

type Plain = { a: number };

const box: Plain = { a: 5 };
let called = false;
const k = ["th", "en"].join("");
(box as any)[k] = (res: (v: number) => void) => {
  called = true;
  res(42);
};

async function main() {
  const v = await box;
  console.log(called, typeof v, (v as any) === box ? "box" : String(v));
}

main();
