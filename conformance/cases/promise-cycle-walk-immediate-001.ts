// A promise settled through the any lane keeps its value as a NaN-box
// even when that value is an immediate (a number, undefined); the exit
// drain's cycle walk must not take such a value for a heap child.

class Mark {
  v: any;
  constructor(v: any) { this.v = v; }
}

// settled with a boxed int
async function readThrough(g: any): Promise<any> {
  const r: any = g.step();
  const t: any = r.v;
  return 3;
}

// settled with undefined, after a yield and an await on a captured promise
async function viaAsyncGenerator(): Promise<string> {
  let release: any;
  const p = new Promise(function (resolve) { release = resolve; });
  async function* f() {
    yield;
    await p;
  }
  const g = f();
  await g.next();
  const second = g.next();
  release();
  const last = await second;
  return JSON.stringify(last);
}

// the awaited value is an owned read of a field on a class instance
async function awaitField(g: any): Promise<any> {
  const r: any = g.step();
  if (r instanceof Mark) {
    return await r.v;
  }
  return "not a mark";
}

async function main() {
  const p = Promise.resolve(1);
  const g = { step: () => ({ v: p }) };
  const q = readThrough(g);
  console.log(await q);

  console.log(await viaAsyncGenerator());

  let release: any;
  const pending = new Promise(function (resolve) { release = resolve; });
  const h = { step() { return new Mark(pending); } };
  const w = awaitField(h);
  release("late");
  console.log(await w);

  const u = (async () => undefined)();
  console.log(await u);
}
main();
