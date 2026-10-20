// The only `then` is stored through Reflect.set reached by
// Function.prototype.apply (ECMA-262 §20.2.3.1).

type Plain = { a: number };

const box: Plain = { a: 5 };
let called = false;

async function main() {
  const v = await box;
  console.log(called, typeof v, (v as any) === box ? "box" : String(v));
}

const f = (res: (v: number) => void) => {
  called = true;
  res(42);
};
Reflect.set.apply(null, [box, "then", f]);
main();
