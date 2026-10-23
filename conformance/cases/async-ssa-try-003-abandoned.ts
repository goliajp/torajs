// An async function abandoned while it waits inside a `try`, a `catch`,
// a `finally` (with a return or a throw pending) or a `for…of` is
// never resumed: no catch block, no finally block and no iterator
// `return()` of it runs, its promise stays pending, and the program
// ends normally. Many such frames in a loop leave nothing behind.

class Held {
  constructor(public tag: string) {}
}

let catches = 0;
let finallies = 0;
let closes = 0;
let after = 0;

function never(): Promise<number> {
  return new Promise<number>(() => {});
}

function counted(): any {
  return {
    [Symbol.iterator]() {
      let i = 0;
      return {
        next() {
          i++;
          return { value: i, done: i > 3 };
        },
        return() {
          closes++;
          return { value: 0, done: true };
        },
      };
    },
  };
}

async function inTry(i: number): Promise<number> {
  const h = new Held("try-" + i);
  try {
    await never();
    after++;
  } catch (e) {
    catches++;
  } finally {
    finallies++;
  }
  return h.tag.length;
}

async function inCatch(i: number): Promise<string> {
  try {
    throw new Held("thrown-" + i);
  } catch (e) {
    const s = "catch-" + i;
    await never();
    after++;
    return s + (e as Held).tag;
  } finally {
    finallies++;
  }
}

async function inFinallyWithReturn(i: number): Promise<string> {
  const s = "ret-" + i;
  try {
    return s + "!";
  } finally {
    await never();
    after++;
  }
}

async function inFinallyWithThrow(i: number): Promise<string> {
  try {
    throw new Held("pending-" + i);
  } finally {
    const t = "fin-" + i;
    await never();
    after++;
    console.log(t);
  }
}

async function inForOf(i: number): Promise<number> {
  let sum = 0;
  try {
    for (const x of counted()) {
      const label = "loop-" + i + "-" + x;
      sum += await never();
      after += label.length;
    }
  } catch (e) {
    catches++;
  }
  return sum;
}

async function main(): Promise<void> {
  for (let i = 0; i < 20000; i++) {
    inTry(i);
    inCatch(i);
    inFinallyWithReturn(i);
    inFinallyWithThrow(i);
    inForOf(i);
  }
  const v = await Promise.resolve(42);
  console.log("catches", catches, "finallies", finallies, "closes", closes, "after", after, v);
}

main();
