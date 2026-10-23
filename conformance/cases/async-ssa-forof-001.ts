// Awaits inside `for…of` bodies: an iterator-protocol loop that throws
// after an await closes its iterator before the rejection, and so do
// `break` and `return` after an await, a rejected await, and a throw
// crossing two loops and a catch; a `return()` that throws loses to
// the original throw. Arrays, strings, Maps and generators keep their
// iteration across awaits, and a loop over an array reads the elements
// another task writes while it waits.

function later<T>(v: T): Promise<T> {
  return Promise.resolve(v);
}

function counted(tag: string, n: number, closeThrows: boolean): any {
  return {
    [Symbol.iterator]() {
      let i = 0;
      return {
        next() {
          i++;
          console.log("next", tag, i);
          return i > n ? { value: 0, done: true } : { value: i, done: false };
        },
        return() {
          console.log("close", tag, i);
          if (closeThrows) throw new Error("close " + tag);
          return { value: 0, done: true };
        },
      };
    },
  };
}

async function throwAfterAwait(): Promise<number> {
  let sum = 0;
  for (const x of counted("throw", 5, false)) {
    sum += await later(x);
    if (x === 2) throw new Error("body " + sum);
  }
  return sum;
}

async function breakAfterAwait(): Promise<number> {
  let sum = 0;
  for (const x of counted("break", 5, false)) {
    sum += await later(x * 10);
    if (x === 3) break;
  }
  return sum;
}

async function returnAfterAwait(): Promise<string> {
  const label = "ret-" + "label";
  for (const x of counted("return", 5, false)) {
    const v = await later(x);
    if (v === 2) return label + ":" + v;
  }
  return "end";
}

async function rejectedAwait(): Promise<number> {
  for (const x of counted("reject", 5, false)) {
    if (x === 2) await Promise.reject(new Error("rejected at " + x));
    await later(x);
  }
  return 0;
}

async function acrossTwoLoopsAndCatch(): Promise<string> {
  try {
    for (const a of counted("outer", 3, false)) {
      for (const b of counted("inner", 3, false)) {
        await later(a + b);
        if (b === 2) throw new Error("deep " + a + b);
      }
    }
    return "none";
  } catch (e) {
    await later(0);
    return "caught " + (e as Error).message;
  }
}

async function closeThrowLoses(): Promise<number> {
  for (const x of counted("bad-close", 5, true)) {
    await later(x);
    throw new Error("original " + x);
  }
  return 0;
}

async function inTryFinally(): Promise<string> {
  const seen: number[] = [];
  try {
    for (const x of counted("finally", 4, false)) {
      seen.push(await later(x));
      if (x === 3) throw new Error("stop");
    }
  } finally {
    await later(0);
    console.log("finally sees", seen.join(","));
  }
  return seen.join(",");
}

async function arrayWrittenWhileWaiting(): Promise<string> {
  const arr: number[] = [1, 2, 3, 4, 5];
  const writer = (async () => {
    for (let i = 0; i < 4; i++) {
      await later(i);
      arr[4 - i] = 100 + i;
    }
  })();
  const seen: number[] = [];
  for (const x of arr) {
    seen.push(x);
    await later(x);
  }
  await writer;
  return seen.join(",") + " / " + arr.join(",");
}

async function others(): Promise<string> {
  const out: string[] = [];
  for (const ch of "héllo") {
    out.push(await later(ch.toUpperCase()));
  }
  const m = new Map<string, number>([
    ["a", 1],
    ["b", 2],
  ]);
  for (const [k, v] of m) {
    out.push(k + (await later(v)));
    if (k === "a") m.set("c", 3);
  }
  function* gen(): Generator<number> {
    try {
      yield 1;
      yield 2;
      yield 3;
    } finally {
      console.log("generator finally");
    }
  }
  try {
    for (const g of gen()) {
      out.push("g" + (await later(g)));
      if (g === 2) throw new Error("gen stop");
    }
  } catch (e) {
    out.push((e as Error).message);
  }
  return out.join(" ");
}

async function main(): Promise<void> {
  for (const f of [throwAfterAwait, rejectedAwait, closeThrowLoses]) {
    try {
      console.log("resolved", await f());
    } catch (e) {
      console.log("rejected", (e as Error).message);
    }
  }
  console.log(await breakAfterAwait());
  console.log(await returnAfterAwait());
  console.log(await acrossTwoLoopsAndCatch());
  try {
    await inTryFinally();
  } catch (e) {
    console.log("rejected", (e as Error).message);
  }
  console.log(await arrayWrittenWhileWaiting());
  console.log(await others());
}

main();
