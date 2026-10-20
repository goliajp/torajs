// A class getter under a computed key held in a binding answers a
// function for `then` (ECMA-262 §15.4.5).

let called = false;
const k = "th" + "en";

class T {
  a = 5;
  get [k]() {
    return (res: (v: number) => void) => {
      called = true;
      res(42);
    };
  }
}

async function main() {
  const t = new T();
  const v = await t;
  console.log(called, typeof v, (v as any) === t ? "box" : String(v));
}

main();
