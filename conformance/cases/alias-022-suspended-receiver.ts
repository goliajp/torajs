// An async method or a generator method reads `this` (and an async
// function or generator reads its parameters) after it has suspended
// and the call that started it has returned. The receiver or argument
// was read out of a place the body then clears; the function's own
// frame (or the generator object) keeps the value alive, as
// §10.2.1.2 OrdinaryCallBindThis and §27.7.5.1 AsyncFunctionStart keep
// the this binding for the whole evaluation. `churn` reallocates cells
// of the same size, so a freed one would answer with another value.

class P {
  v: number;
  w: number;
  constructor(v: number) {
    this.v = v;
    this.w = v * 2;
  }
  async run(): Promise<number> {
    box.p = null;
    await null;
    churn();
    return this.v + this.w;
  }
  async late(): Promise<number> {
    await null;
    box.p = null;
    churn();
    return this.v;
  }
  async lateIdx(): Promise<number> {
    await null;
    arr[0] = null;
    churn();
    return this.v;
  }
  async tryLate(): Promise<number> {
    try {
      await null;
      box.p = null;
      churn();
      return this.v;
    } finally {
      churn();
    }
  }
  async str(s: string): Promise<string> {
    await null;
    box.s = "";
    churnS();
    return s.slice(0, 6) + this.v;
  }
  *gen(): Generator<number> {
    yield this.v;
    box.p = null;
    churn();
    yield this.w;
  }
  *genEarly(): Generator<number> {
    box.p = null;
    churn();
    yield this.v;
  }
}
class Box {
  p: P | null = null;
  s: string = "";
}
const box = new Box();
const keep: P[] = [];
const keepS: string[] = [];
function churn(): void {
  for (let i = 0; i < 4; i++) keep.push(new P(900 + i));
}
function churnS(): void {
  for (let i = 0; i < 4; i++) keepS.push("zzzzzzzzzzzzzzzzzzzzzzzzzzzzz" + i);
}
const arr: (P | null)[] = [null];

async function arg(q: P): Promise<number> {
  box.p = null;
  await null;
  churn();
  return q.v;
}
function* sgen(q: P): Generator<number> {
  box.p = null;
  churn();
  yield q.v;
  yield q.w;
}
async function forAwait(q: P): Promise<number> {
  let t = 0;
  for await (const x of [1, 2]) {
    box.p = null;
    churn();
    t += x + q.v;
  }
  return t;
}

async function main(): Promise<void> {
  box.p = new P(7);
  console.log("run", await box.p!.run());
  box.p = new P(8);
  console.log("late", await box.p!.late());
  arr[0] = new P(9);
  console.log("element", await arr[0]!.lateIdx());
  box.p = new P(10);
  console.log("try", await box.p!.tryLate());
  box.p = new P(11);
  box.s = "abcdefghijklmnopqrstuvwxyz-" + String(keep.length);
  console.log("str", await box.p!.str(box.s));
  box.p = new P(12);
  const g = box.p!.gen();
  for (const x of g) console.log("gen", x);
  box.p = new P(13);
  for (const x of box.p!.genEarly()) console.log("gen early", x);
  box.p = new P(14);
  console.log("arg", await arg(box.p!));
  box.p = new P(16);
  for (const x of sgen(box.p!)) console.log("gen arg", x);
  box.p = new P(17);
  console.log("for await", await forAwait(box.p!));
  const q = new P(18);
  box.p = q;
  const f = async (): Promise<number> => {
    await null;
    box.p = null;
    churn();
    return q.v;
  };
  console.log("arrow", await f());
}
main();
