// A class method under a computed key that evaluates to "then"
// (ECMA-262 §15.4.4): an instance is a thenable though no member of
// the class is spelled `then`.

let called = false;

class T {
  a = 5;
  ["th" + "en"](res: (v: number) => void) {
    called = true;
    res(42);
  }
}

async function main() {
  const t = new T();
  const v = await t;
  console.log(called, typeof v, (v as any) === t ? "box" : String(v));
}

main();
