// Guards the substrate shapes the test262 asyncHelpers.js port
// (conformance/test262-harness.ts __t262_asyncTest /
// __t262_throwsAsync, 2026-07-30) depends on: calling an any-typed
// thunk, hooking .then with two closure callbacks, a two-parameter
// Promise executor whose resolvers escape into outer lets, resolving
// a promise WITH a promise (state adoption), and comparing
// `thrown.constructor` against a class object by identity.

function wrap(f: any): void {
  if (typeof f !== "function") {
    console.log("non-function");
    return;
  }
  try {
    const p: any = f();
    p.then(
      function (): void {
        console.log("fulfilled");
      },
      function (e: any): void {
        console.log("rejected:" + e.message);
      }
    );
  } catch (syncError) {
    const se: any = syncError;
    console.log("sync:" + se.message);
  }
}

function expectAsyncThrow(ctor: any, f: any): any {
  return new Promise(function (resolve: any): void {
    const res: any = f();
    let onOk: any = undefined;
    let onErr: any = undefined;
    const settled: any = new Promise(function (a: any, b: any): void {
      onOk = a;
      onErr = b;
    });
    res.then(onOk, onErr);
    resolve(
      settled.then(
        function (): void {
          throw new Error("expected rejection, got fulfillment");
        },
        function (thrown: any): void {
          if (thrown.constructor !== ctor) {
            const actualName: any = thrown.constructor.name;
            throw new Error("wrong ctor: " + actualName);
          }
        }
      )
    );
  });
}

async function main(): Promise<void> {
  wrap(42);
  wrap(async function (): Promise<void> {});
  wrap(async function (): Promise<void> {
    throw new Error("boom");
  });
  wrap(function (): any {
    throw new Error("early");
  });
  await expectAsyncThrow(TypeError, async function (): Promise<void> {
    throw new TypeError("t");
  });
  console.log("ctor-match ok");
  try {
    await expectAsyncThrow(TypeError, async function (): Promise<void> {
      throw new RangeError("r");
    });
    console.log("BAD: mismatch accepted");
  } catch (e) {
    const ee: any = e;
    console.log("mismatch caught: " + ee.message);
  }
  try {
    await expectAsyncThrow(TypeError, async function (): Promise<void> {});
    console.log("BAD: fulfillment accepted");
  } catch (e) {
    const ee: any = e;
    console.log("fulfillment caught: " + ee.message);
  }
  console.log("done");
}
main();
