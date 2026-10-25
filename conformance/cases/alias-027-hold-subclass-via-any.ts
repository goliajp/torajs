// A subclass instance that reaches a base-typed slot only through a
// call made by way of an `any` (the boxed adapter decodes it behind
// its layout test) is still known to sit there: a cell or a string
// read out of it and handed to a function that clears the slot lives
// through the call. `churn` reallocates cells and strings of the same
// sizes, so a freed one would answer with another value.

class Inner {
  v: number = 0;
  w: number = 0;
}
class Base {
  k: number = 1;
}
class Sub extends Base {
  inner: Inner = new Inner();
  name: string = "";
}
let slot: Base | null = null;
function keep(b: Base): void {
  slot = b;
}
const keepI: Inner[] = [];
const keepS: string[] = [];
function churn(): void {
  for (let i = 0; i < 4; i++) {
    const x = new Inner();
    x.v = 900 + i;
    x.w = 901;
    keepI.push(x);
    keepS.push("zzzzzzzzzzzzzzzzzzzzzzzzzz" + i);
  }
}
function useI(i: Inner): number {
  slot = null;
  churn();
  return i.v * 10 + i.w;
}
function useS(s: string): string {
  slot = null;
  churn();
  return s.slice(2) + " " + s.length;
}
function make(v: number, w: number): Sub {
  const s = new Sub();
  s.inner.v = v;
  s.inner.w = w;
  s.name = "n" + String(v) + "abcdefghijklmnopqrstuvwxy";
  return s;
}

function main(): void {
  const f: any = keep;
  f(make(Number(String(Date.now()).slice(0, 1)), 7));
  console.log("cast cell", useI((slot as Sub).inner));
  f(make(3, 4));
  if (slot instanceof Sub) {
    console.log("instanceof cell", useI(slot.inner));
  }
  f(make(5, 6));
  console.log("cast string", useS((slot as Sub).name));
  f(make(7, 8));
  if (slot instanceof Sub) {
    console.log("instanceof string", useS(slot.name));
  }
  console.log("cleared", slot === null);
}
main();
