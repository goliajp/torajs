// A class whose base is read at run time stores `super.then = f` on
// the instance (ECMA-262 §10.1.9.2); an await of the instance calls it.

let called = false;
const bases: any[] = [class {}];

class Box extends bases[0] {
  a = 5;
  arm(f: (res: (v: number) => void) => void) {
    super.then = f;
  }
}

async function main() {
  const b = new Box();
  b.arm((res) => {
    called = true;
    res(42);
  });
  const v = await b;
  console.log(called, typeof v, (v as any) === b ? "box" : String(v));
}

main();
