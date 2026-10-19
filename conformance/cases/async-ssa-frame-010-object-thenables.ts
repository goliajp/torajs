// `await` of an object reads its `then` when it runs (ECMA-262
// §27.7.5.3, §27.2.1.3.2): a class instance whose class or subclass
// has one, an array, a function or a plain object given one later all
// settle through it; one without settles as itself.

class T {
  then(r: (v: number) => void): void {
    r(42);
  }
}

class Base {
  x: number = 1;
}

class Derived extends Base {
  then(r: (v: number) => void): void {
    r(7);
  }
}

class Plain {
  y: number = 3;
}

const later = new Plain();
(later as any).then = (r: any) => r(5);

function g(): number {
  return 1;
}
(g as any).then = (r: any) => r(3);

const thenArr: number[] = [1, 2];
(thenArr as any).then = (r: any) => r(9);

async function main(): Promise<void> {
  const v = await new T();
  console.log("class", v);
  const b: Base = new Derived();
  console.log("subclass", await b);
  const p = await new Plain();
  console.log("plain", (p as Plain).y);
  console.log("expando object", await later);
  console.log("expando function", await g);
  console.log("expando array", await thenArr);
  const arr = [4, 5, 6];
  const u = await arr;
  console.log("array", (u as number[]).length);
  const fn = (x: number) => x + 1;
  const fv: (x: number) => number = await fn;
  console.log("function", fv(1));
  const o = { k: 5 };
  console.log("object", (await o).k);
  const m = new Map<string, number>();
  m.set("a", 1);
  console.log("map", (await m).get("a"));
}

main();
