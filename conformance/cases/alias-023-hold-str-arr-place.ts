// A string or an array read out of a place (a field, an element, a
// reassigned top-level binding) and handed to a user function, or used
// as the receiver of an array method that runs a callback, lives
// through the call even when the callee or the callback overwrites the
// place. `churn` reallocates cells of the same
// sizes, so a freed one would answer with another value.

class Box {
  s: string = "";
  xs: number[] = [];
  ps: string[] = [];
}
const box = new Box();
const keepS: string[] = [];
const keepA: number[][] = [];
function churn(): void {
  for (let i = 0; i < 4; i++) {
    keepS.push("zzzzzzzzzzzzzzzzzzzzzzzzzzzzz" + i);
    keepA.push([900 + i, 901, 902, 903]);
  }
}

function useS(s: string): number {
  box.s = "";
  churn();
  return s.length * 1000 + s.charCodeAt(3);
}
function useA(xs: number[]): number {
  box.xs = [];
  churn();
  return xs[0] + xs[3];
}
let gs: string = "";
let ga: number[] = [];
function useGS(s: string): string {
  gs = "";
  churn();
  return s.slice(0, 8);
}
function useGA(xs: number[]): number {
  ga = [];
  churn();
  return xs[1];
}
const rows: number[][] = [[1, 2, 3, 4]];
function useRow(xs: number[]): number {
  rows[0] = [];
  churn();
  return xs[2];
}
function throwA(xs: number[]): number {
  box.xs = [];
  churn();
  if (xs[0] === 41) throw new Error("t" + xs[1]);
  return 0;
}

class Holder {
  items: number[] = [];
  name: string = "";
  sumItems(xs: number[]): number {
    this.items = [];
    churn();
    return xs[0] + xs[1] + xs[2] + xs[3];
  }
  greet(s: string): string {
    this.name = "";
    churn();
    return "hi " + s.slice(0, 5);
  }
}

let total = 0;
function main(): void {
  box.s = "abcdefghijklmnopqrstuvwxyz0123456789-" + String(total);
  console.log("str field", useS(box.s));
  box.xs = [5, 6, 7, 8];
  console.log("arr field", useA(box.xs));
  gs = "hello-world-" + String(total + 1) + "-padpadpadpadpadpad";
  console.log("str global", useGS(gs));
  ga = [11, 12, 13, 14];
  console.log("arr global", useGA(ga));
  console.log("arr element", useRow(rows[0]));
  box.xs = [41, 42, 43, 44];
  try {
    throwA(box.xs);
  } catch (e) {
    console.log("arr throw", (e as Error).message);
  }
  const h = new Holder();
  h.items = [1, 2, 3, 4];
  console.log("method arr", h.sumItems(h.items));
  h.name = "world-" + String(total) + "-padpadpadpadpadpadpad";
  console.log("method str", h.greet(h.name));

  box.xs = [21, 22, 23, 24];
  const m = box.xs.map((x: number): number => {
    if (x === 21) {
      box.xs = [];
      churn();
    }
    return x + 1;
  });
  console.log("map", m.join(","));
  box.xs = [31, 32, 33, 34];
  box.xs.forEach((x: number): void => {
    if (x === 31) {
      box.xs = [];
      churn();
    }
    total += x;
  });
  console.log("forEach", total);
  box.xs = [61, 62, 63, 64];
  const r = box.xs.reduce((a: number, x: number): number => {
    if (x === 61) {
      box.xs = [];
      churn();
    }
    return a + x;
  }, 0);
  console.log("reduce", r);
  box.xs = [71, 72, 73, 74];
  const found = box.xs.find((x: number): boolean => {
    box.xs = [];
    churn();
    return x === 73;
  });
  console.log("find", found);
  box.xs = [81, 82, 83, 84];
  const all = box.xs.every((x: number): boolean => {
    box.xs = [];
    churn();
    return x > 80;
  });
  console.log("every", all);
  box.xs = [91, 92, 93, 94];
  const idx = box.xs.findIndex((x: number): boolean => {
    box.xs = [];
    churn();
    return x === 94;
  });
  console.log("findIndex", idx);
  box.xs = [1, 2, 3, 4];
  const flat = box.xs.flatMap((x: number): number[] => {
    box.xs = [];
    churn();
    return [x, x * 10];
  });
  console.log("flatMap", flat.join(","));
  box.ps = ["a1", "b2", "c3"];
  const kept = box.ps.filter((x: string): boolean => {
    box.ps = [];
    churn();
    return x !== "b2";
  });
  console.log("filter", kept.join(","));
}
main();

function useE(s: string): string {
  box.ps = [];
  churn();
  return s;
}
function app(s: string): number {
  box.s += "!!!!";
  return s.length;
}

// a subclass instance in a base-typed slot holds its own fields; a
// release of the slot frees them
class Base {
  k: number = 0;
  runS(): string {
    return "";
  }
  runA(): number {
    return 0;
  }
}
class Sub extends Base {
  name: string;
  xs: number[];
  constructor(n: string) {
    super();
    this.name = n;
    this.xs = [1, 2, 3, 4];
  }
  runS(): string {
    return useSub(this.name);
  }
  runA(): number {
    return useSubA(this.xs);
  }
}
let g: Base = new Sub("abcdefghijklmnopqrstuvwxyz-" + String(keepS.length));
let g2: Base = new Sub("q");
function useSub(s: string): string {
  g = new Base();
  churn();
  return s.slice(0, 8);
}
function useSubA(xs: number[]): number {
  g2 = new Base();
  churn();
  return xs[3];
}

function more(): void {
  box.ps = ["elem-" + String(total) + "-padpadpadpadpadpad"];
  console.log("str element", useE(box.ps[0]));
  box.s = "abc" + String(total);
  console.log("append", app(box.s), box.s);
  box.xs = [1, 2, 3, 4];
  try {
    box.xs.forEach((x: number): void => {
      box.xs = [];
      churn();
      if (x === 2) throw new Error("cb" + x);
    });
  } catch (e) {
    console.log("cb throw", (e as Error).message);
  }
  rows[0] = [5, 6, 7, 8];
  const er = rows[0].map((x: number): number => {
    rows[0] = [];
    churn();
    return x;
  });
  console.log("elem recv", er.join(","));
  console.log("subclass str", g.runS());
  console.log("subclass arr", g2.runA());
}
more();
