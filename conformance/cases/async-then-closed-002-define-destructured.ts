// The only `then` is defined through defineProperty destructured off
// Reflect (ECMA-262 §28.1.3).

type Plain = { a: number };

const box: Plain = { a: 5 };
let called = false;

async function main() {
  const v = await box;
  console.log(called, typeof v, (v as any) === box ? "box" : String(v));
}

const { defineProperty } = Reflect;
defineProperty(box, "then", { value: (res: (v: number) => void) => {
  called = true;
  res(42);
} });
main();
