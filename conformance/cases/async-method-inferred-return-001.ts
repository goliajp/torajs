// An `async` class method with no return annotation was rejected
// outright:
//
//     async class method `C::load` requires an explicit return type
//     annotation `: T` or `: Promise<T>` (P10.3-A3a MVP — mirrors
//     top-level `async function f(): T` requirement)
//
// The requirement it claimed to mirror was gone: a top-level `async
// function f() { return 7 }` has defaulted to `Promise<any>` since
// P10.7. The class path held a second copy of the same body transform
// and only the first copy was taught the default.
//
// Also here: `async function f(): Promise<void> {}`. Its tail-safety
// return took the catch-all zero, so a function promising nothing was
// caught returning a number — `void` now takes the same `undefined`
// the untyped case already took.

class Service {
  v = 5;
  s = "hi";
  items: number[] = [1, 2, 3];

  async num() {
    return this.v;
  }
  async str() {
    return this.s;
  }
  async list() {
    return this.items;
  }
  // no return at all — the tail-safety return is the only one
  async bump() {
    this.v = this.v + 1;
  }
  // awaits a sibling, then computes
  async chain() {
    const a = await this.num();
    return a * 2;
  }
  // returns on one path and falls off the other. Only the returning
  // path is exercised: a value read off the fall-off path prints
  // `[unknown-any-tag]` where bun prints `undefined`, which is a
  // pre-existing hole in the untyped tail return (the plain sync
  // `function g(f) { if (f) return 1 }` mis-prints too) and is
  // recorded rather than locked in here.
  async branch(flag: boolean) {
    if (flag) {
      return this.v;
    }
  }
  static async stat() {
    return 11;
  }
  // the annotated forms are unchanged ground
  async annotated(): Promise<number> {
    return this.v;
  }
  async bareAnn(): number {
    return this.v + 1;
  }
  async voidAnn(): Promise<void> {
    this.s = this.s + "!";
  }
}

// a top-level async fn promising nothing, with and without a tail
// return
async function nothing(): Promise<void> {
  console.log("ran");
}
async function nothingReturns(): Promise<void> {
  console.log("ran2");
  return;
}
async function inferred() {
  return 7;
}

async function main(): Promise<void> {
  const s = new Service();
  console.log(await s.num(), await s.str());
  console.log((await s.list()).length);
  await s.bump();
  console.log(s.v);
  console.log(await s.chain());
  console.log(await s.branch(true));
  console.log(await Service.stat());
  console.log(await s.annotated(), await s.bareAnn());
  await s.voidAnn();
  console.log(s.s);
  await nothing();
  await nothingReturns();
  console.log(await inferred());
}

main();
