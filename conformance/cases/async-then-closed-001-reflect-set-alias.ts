// The only `then` is stored through Reflect.set held in a binding
// (ECMA-262 §28.1.12, §27.2.1.3.2 steps 9-13).

type Plain = { a: number };

const box: Plain = { a: 5 };
let called = false;

async function main() {
  const v = await box;
  console.log(called, typeof v, (v as any) === box ? "box" : String(v));
}

const rs = Reflect.set;
rs(box, "then", (res: (v: number) => void) => {
  called = true;
  res(42);
});
main();
