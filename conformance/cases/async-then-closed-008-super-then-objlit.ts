// `super.then = f` in an object literal's method stores on the receiver
// (ECMA-262 §10.1.9.2 step 2.c-e); an await of the object calls it.

let called = false;
const box = {
  a: 5,
  arm(f: (res: (v: number) => void) => void) {
    super.then = f;
  },
};

async function main() {
  box.arm((res) => {
    called = true;
    res(42);
  });
  const v = await box;
  console.log(called, typeof v, (v as any) === box ? "box" : String(v));
}

main();
