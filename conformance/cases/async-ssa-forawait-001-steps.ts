// An async function's `for await` steps and closes suspend its frame: an
// array (a break, a throw out of the body, a rejected element), a class
// async iterator (break and throw run its `return()`), a sync iterator
// under the Async-from-Sync wrapper, an async generator left by `return`,
// a Map and a string, each against the job queue.
const log: string[] = [];
function ticks(tag: string, n: number) {
  let p = Promise.resolve();
  for (let i = 1; i <= n; i++) {
    p = p.then(() => { log.push(tag + i); });
  }
  return p;
}

async function arrBreak() {
  for await (const x of [1, Promise.resolve(2), 3]) {
    log.push("ab" + x);
    if (x === 2) break;
  }
  log.push("ab-end");
}

async function arrThrow() {
  try {
    for await (const x of [Promise.resolve(1), 2]) {
      log.push("at" + x);
      throw new Error("boom");
    }
  } catch (e: any) {
    log.push("at-caught " + e.message);
  }
}

async function arrRejected() {
  try {
    for await (const x of [1, Promise.reject(new Error("rej")), 3]) {
      log.push("ar" + x);
    }
  } catch (e: any) {
    log.push("ar-caught " + e.message);
  }
}

class AsyncIt {
  i = 0;
  [Symbol.asyncIterator]() { return this; }
  next() {
    this.i++;
    log.push("ai-next" + this.i);
    return Promise.resolve({ value: this.i, done: this.i > 3 });
  }
  return() {
    log.push("ai-return");
    return Promise.resolve({ value: undefined, done: true });
  }
}

async function classBreak() {
  for await (const x of new AsyncIt()) {
    log.push("cb" + x);
    if (x === 2) break;
  }
  log.push("cb-end");
}

async function classThrow() {
  try {
    for await (const x of new AsyncIt()) {
      log.push("ct" + x);
      throw new Error("ct-boom");
    }
  } catch (e: any) {
    log.push("ct-caught " + e.message);
  }
}

async function syncIterReturn() {
  const it: any = {
    [Symbol.iterator]() {
      let i = 0;
      return {
        next() { i++; return { value: Promise.resolve(i * 10), done: i > 3 }; },
        return() { log.push("si-return"); return { done: true }; },
      };
    },
  };
  for await (const x of it) {
    log.push("si" + x);
    if (x === 20) break;
  }
  log.push("si-end");
}

async function* gen() {
  yield 1;
  yield 2;
  yield 3;
}

async function genLoop() {
  for await (const v of gen()) {
    log.push("g" + v);
    if (v === 2) return "early";
  }
  return "late";
}

async function mapLoop() {
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  for await (const [k, v] of m) log.push("m" + k + v);
  for await (const c of "xy") log.push("s" + c);
}

async function main() {
  ticks("A", 8);
  await arrBreak();
  ticks("B", 8);
  await arrThrow();
  ticks("C", 8);
  await arrRejected();
  ticks("D", 10);
  await classBreak();
  ticks("E", 10);
  await classThrow();
  ticks("F", 12);
  await syncIterReturn();
  ticks("G", 12);
  log.push("gen " + (await genLoop()));
  ticks("H", 8);
  await mapLoop();
  await ticks("Z", 1);
  console.log(log.join(" "));
}
main();
