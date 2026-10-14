// A function an async function declares under a builtin's name is the
// local binding in every closure below it, `new` included.

async function viaAsyncExpr() {
  function TypeError() {}
  const f = async function () {
    return new (TypeError as any)();
  };
  console.log((await f()).constructor === TypeError);
}

async function viaAsyncArrow() {
  function RangeError(this: any) {
    this.tag = "local";
  }
  await null;
  const f = async () => new (RangeError as any)();
  const r = await f();
  console.log(r.constructor === RangeError, r.tag);
}

async function viaGeneratorClass() {
  function Error(this: any) {
    this.tag = "mine";
  }
  await null;
  class K {
    make() {
      const g = () => new (Error as any)();
      return g();
    }
  }
  const e = new K().make();
  console.log(e.constructor === Error, e.tag);
}

async function builtinStillBuiltin() {
  await null;
  const f = async () => new TypeError("x");
  const e = await f();
  console.log(e instanceof Error, e.message);
}

async function main() {
  await viaAsyncExpr();
  await viaAsyncArrow();
  await viaGeneratorClass();
  await builtinStillBuiltin();
}
main();
