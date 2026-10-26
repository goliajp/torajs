// An async function's `for await` over `any` sources: an async iterator
// whose `return()` rejects (the body's own throw wins, a break raises the
// rejection), a sync iterator with a rejected value and a throwing
// `return`, an array and a Set, each against the job queue.
const log: string[] = [];
function ticks(tag: string, n: number) {
  let p = Promise.resolve();
  for (let i = 1; i <= n; i++) {
    p = p.then(() => { log.push(tag + i); });
  }
  return p;
}

function asyncIt(retRejects: boolean): any {
  let i = 0;
  return {
    [Symbol.asyncIterator]() {
      return {
        next() { i++; log.push("n" + i); return Promise.resolve({ value: i, done: i > 3 }); },
        return() {
          log.push("ret");
          return retRejects ? (Promise.reject(new Error("ret-rej")) as any) : (Promise.resolve({ done: true }) as any);
        },
      };
    },
  };
}

function syncIt(retThrows: boolean): any {
  return {
    [Symbol.iterator]() {
      let i = 0;
      return {
        next() { i++; return { value: i === 2 ? (Promise.reject(new Error("v-rej")) as any) : (i as any), done: i > 3 }; },
        return() {
          log.push("sret");
          if (retThrows) throw new Error("sret-throw");
          return { done: true, value: Promise.resolve(9) };
        },
      };
    },
  };
}

async function a1(src: any) {
  try {
    for await (const x of src) {
      log.push("a1:" + x);
      if (x === 2) throw new Error("body");
    }
  } catch (e: any) {
    log.push("a1-caught " + e.message);
  }
}

async function a2(src: any) {
  for await (const x of src) {
    log.push("a2:" + x);
    if (x === 1) break;
  }
  log.push("a2-end");
}

async function a3(src: any) {
  try {
    for await (const x of src) log.push("a3:" + x);
  } catch (e: any) {
    log.push("a3-caught " + e.message);
  }
}

async function a4(src: any) {
  let s = 0;
  for await (const x of src) s += x;
  return s;
}

async function main() {
  ticks("A", 10);
  await a1(asyncIt(true));
  ticks("B", 10);
  await a2(asyncIt(false));
  ticks("C", 10);
  try { await a2(asyncIt(true)); } catch (e: any) { log.push("a2-rej " + e.message); }
  ticks("D", 10);
  await a3(syncIt(false));
  ticks("E", 10);
  await a2(syncIt(false));
  ticks("F", 10);
  try { await a2(syncIt(true)); } catch (e: any) { log.push("a2s-rej " + e.message); }
  ticks("G", 10);
  await a1(syncIt(true));
  ticks("H", 10);
  const arr: any = [1, Promise.resolve(2), 3];
  log.push("a4 " + (await a4(arr)));
  const m: any = new Set([4, 5]);
  log.push("a4s " + (await a4(m)));
  await ticks("Z", 1);
  console.log(log.join(" "));
}
main();
