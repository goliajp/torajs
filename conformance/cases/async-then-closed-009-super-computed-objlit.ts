// `super[k] = f` under a computed key that evaluates to "then".

let called = false;
const box = {
  a: 5,
  arm(k: string, f: (res: (v: number) => void) => void) {
    super[k] = f;
  },
};

async function main() {
  box.arm("th" + "en", (res) => {
    called = true;
    res(42);
  });
  const v = await box;
  console.log(called, typeof v, (v as any) === box ? "box" : String(v));
}

main();
