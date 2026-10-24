// An async function settles its own result: a return does not call
// `Promise.resolve` and a throw does not call `Promise.reject`
// (§27.7.5.2 AsyncBlockStart), so replacing them is never observed.
// The receiver-polymorphic twin of an async method (reached through an
// `any` receiver) and a generic class's method settle the same way.
const origResolve = Promise.resolve;
const origReject = Promise.reject;
(Promise as any).resolve = (v: any) => {
  console.log("user resolve");
  return origResolve.call(Promise, v);
};
(Promise as any).reject = (v: any) => {
  console.log("user reject");
  return origReject.call(Promise, v);
};

class K {
  base = 10;
  async plain(x: number): Promise<number> {
    return x + this.base;
  }
  async waits(x: number): Promise<number> {
    const y = await x;
    return y + this.base;
  }
  async raises(x: number): Promise<number> {
    if (x > 0) throw new Error("raises " + x);
    return x;
  }
  async raisesLate(x: number): Promise<number> {
    await null;
    if (x > 0) throw new Error("raisesLate " + x);
    return x;
  }
}

class Box<T> {
  constructor(public v: T) {}
  async get(): Promise<T> {
    return this.v;
  }
  async getLate(): Promise<T> {
    await null;
    return this.v;
  }
  async fail(): Promise<T> {
    throw new Error("box fail");
  }
}

async function run() {
  const k = new K();
  const a: any = k;
  console.log(await k.plain(1), await a.plain(2));
  console.log(await k.waits(3), await a.waits(4));
  for (const r of [k, a]) {
    try {
      await r.raises(5);
    } catch (e: any) {
      console.log("caught " + e.message);
    }
    try {
      await r.raisesLate(6);
    } catch (e: any) {
      console.log("caught " + e.message);
    }
  }
  const b = new Box<number>(7);
  const s = new Box<string>("s");
  const ab: any = b;
  console.log(await b.get(), await s.get(), await ab.get());
  console.log(await b.getLate(), await s.getLate(), await ab.getLate());
  try {
    await ab.fail();
  } catch (e: any) {
    console.log("caught " + e.message);
  }
  try {
    await s.fail();
  } catch (e: any) {
    console.log("caught " + e.message);
  }
}
run().then(() => console.log("done"));
