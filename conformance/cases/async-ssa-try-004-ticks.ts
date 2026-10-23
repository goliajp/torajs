// Job order of awaits inside `try` statements and `for…of` loops,
// interleaved with a `.then` chain: an await of a non-thenable and of
// a settled promise inside try / catch / finally, `return p` and
// `return await p` from a try block (the second is caught by the
// catch), and a rejection that reaches the catch.

const log: string[] = [];

function chain(n: number): Promise<void> {
  let p: Promise<void> = Promise.resolve();
  for (let i = 0; i < n; i++) {
    p = p.then(() => {
      log.push("t" + i);
    });
  }
  return p;
}

async function awaits(): Promise<string> {
  try {
    await null;
    log.push("try-1");
    await Promise.resolve(1);
    log.push("try-2");
    throw new Error("x");
  } catch (e) {
    await 0;
    log.push("catch-1");
  } finally {
    await Promise.resolve("f");
    log.push("finally-1");
  }
  for (const x of [1, 2]) {
    await x;
    log.push("loop-" + x);
  }
  return "awaits";
}

async function returnPromise(): Promise<string> {
  try {
    return Promise.resolve("p");
  } finally {
    log.push("rp-finally");
  }
}

async function returnAwaitCaught(): Promise<string> {
  try {
    return await Promise.reject(new Error("r"));
  } catch (e) {
    log.push("rac-catch");
    return "caught";
  }
}

async function rejectionCaught(): Promise<string> {
  try {
    await Promise.reject(new Error("rej"));
    return "no";
  } catch (e) {
    log.push("rc-catch");
    return (e as Error).message;
  }
}

async function main(): Promise<void> {
  const results = await Promise.all([
    chain(12),
    awaits().then((v) => log.push("done " + v)),
    returnPromise().then((v) => log.push("done rp " + v)),
    returnAwaitCaught().then((v) => log.push("done rac " + v)),
    rejectionCaught().then((v) => log.push("done rc " + v)),
  ]);
  console.log(results.length);
  console.log(log.join("\n"));
}

main();
