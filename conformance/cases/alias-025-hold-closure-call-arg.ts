// A class instance, a string or an array read out of a place (a
// field, an element, a reassigned top-level binding, a binding a
// closure shares) and handed to a closure value lives through the
// call even when the closure, or one it runs, overwrites the place.
// `churn` reallocates cells of the same sizes, so a freed one would
// answer with another value.

class P {
  v: number;
  w: number;
  constructor(v: number) {
    this.v = v;
    this.w = v * 2;
  }
}
class Box {
  p: P | null = null;
  s: string = "";
  xs: number[] = [];
}
const box = new Box();
const keep: P[] = [];
const keepS: string[] = [];
const keepA: number[][] = [];
function churn(): void {
  for (let i = 0; i < 4; i++) {
    keep.push(new P(900 + i));
    keepS.push("zzzzzzzzzzzzzzzzzzzzzzzzzzzzz" + i);
    keepA.push([900 + i, 901, 902, 903]);
  }
}

const useP = (q: P): number => {
  box.p = null;
  churn();
  return q.v;
};
const useS = (s: string): string => {
  box.s = "";
  churn();
  return s.slice(0, 7);
};
const useA = (xs: number[]): number => {
  box.xs = [];
  churn();
  return xs[0] + xs[3];
};
const items: P[] = [new P(0)];
let gp: P = new P(0);

function main(): void {
  box.p = new P(7);
  console.log("closure field obj", useP(box.p!));
  box.s = "abcdefghijklmnopqrstuvwxyz-" + String(keep.length);
  console.log("closure field str", useS(box.s));
  box.xs = [5, 6, 7, 8];
  console.log("closure field arr", useA(box.xs));

  items[0] = new P(9);
  const atIdx = (q: P): number => {
    items[0] = new P(1);
    churn();
    return q.w;
  };
  console.log("closure element", atIdx(items[0]));

  gp = new P(10);
  const atGlobal = (q: P): number => {
    gp = new P(1);
    churn();
    return q.v;
  };
  console.log("closure global", atGlobal(gp));

  let shared: P = new P(11);
  let sharedA: number[] = [51, 52, 53, 54];
  const clear = (): void => {
    shared = new P(1);
    sharedA = [];
    churn();
  };
  const viaBox = (q: P, xs: number[], f: () => void): number => {
    f();
    return q.v + xs[3];
  };
  console.log("capture box", viaBox(shared, sharedA, clear));

  const fns: ((q: P) => number)[] = [useP];
  box.p = new P(12);
  console.log("closure from array", fns[0](box.p!));
}
main();
