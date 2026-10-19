// A parameter default that throws rejects the async function's promise
// (§15.8.4): the default is evaluated inside the function, so nothing
// is thrown at the caller. Rest parameters arrive packed and survive
// the suspension.

function fail(): number {
  throw new Error("default failed");
}

async function withDefault(a: number, b: any = fail()): Promise<number> {
  await null;
  return a + b;
}

async function sum(...xs: number[]): Promise<number> {
  let t = 0;
  for (let i = 0; i < xs.length; i++) {
    await null;
    t += xs[i];
  }
  return t;
}

async function main(): Promise<void> {
  let threw = false;
  let p: Promise<number> = Promise.resolve(0);
  try {
    p = withDefault(1);
  } catch (e: any) {
    threw = true;
  }
  console.log("sync throw", threw);
  console.log(await p.catch((e: any) => e.message));
  console.log(await withDefault(1, 2));
  console.log(await sum(1, 2, 3, 4));
  console.log(await sum());
}

main();
