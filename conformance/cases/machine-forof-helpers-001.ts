// for-of and for-await inside state machines (generators, async
// functions, async generators) over arrays, Sets, Maps, strings and a
// user iterator, with break / return / throw leaving the loop, so the
// iterator-protocol helpers must be present (§14.7.5.6-7, §7.4.9,
// §7.4.11)

function makeIter(n: number, log: string[]): any {
  let i = 0;
  return {
    [Symbol.iterator]() {
      return this;
    },
    next() {
      i++;
      return i <= n ? { value: i * 10, done: false } : { value: undefined, done: true };
    },
    return() {
      log.push("close@" + i);
      return { value: undefined, done: true };
    },
  };
}

function* overArray(xs: number[]): Generator<number> {
  for (const x of xs) yield x * 2;
}

function* overSet(s: Set<string>): Generator<string> {
  for (const v of s) yield v.toUpperCase();
}

function* overMap(m: Map<string, number>): Generator<string> {
  for (const [k, v] of m) yield k + "=" + v;
}

function* overString(s: string): Generator<string> {
  for (const c of s) yield "<" + c + ">";
}

function* overSplit(s: string): Generator<string> {
  for (const w of s.split(",")) yield w.trim();
}

function* breakOut(log: string[]): Generator<number> {
  for (const v of makeIter(5, log)) {
    if (v > 20) break;
    yield v;
  }
  yield -1;
}

function* returnOut(log: string[]): Generator<number> {
  for (const v of makeIter(5, log)) {
    yield v;
    if (v === 20) return 99;
  }
  return 0;
}

function* throwOut(log: string[]): Generator<number> {
  for (const v of makeIter(5, log)) {
    yield v;
    if (v === 30) throw new Error("boom at " + v);
  }
}

async function sumAsync(xs: number[]): Promise<number> {
  let t = 0;
  for (const x of xs) {
    t += await Promise.resolve(x);
  }
  return t;
}

async function breakAsync(log: string[]): Promise<number> {
  let last = 0;
  for (const v of makeIter(5, log)) {
    last = await Promise.resolve(v);
    if (last >= 30) break;
  }
  return last;
}

async function awaitLoop(): Promise<string> {
  const ps = [Promise.resolve("a"), Promise.resolve("b"), "c"];
  let out = "";
  for await (const v of ps) out += v;
  return out;
}

async function* agen(xs: number[]): AsyncGenerator<number> {
  for (const x of xs) {
    yield await Promise.resolve(x + 1);
  }
}

async function main(): Promise<void> {
  console.log([...overArray([1, 2, 3])].join(" "));
  console.log([...overSet(new Set(["a", "b", "a"]))].join(" "));
  const m = new Map<string, number>();
  m.set("x", 1);
  m.set("y", 2);
  console.log([...overMap(m)].join(" "));
  console.log([...overString("hey")].join(""));
  console.log([...overSplit("p, q ,r")].join("|"));

  const l1: string[] = [];
  console.log([...breakOut(l1)].join(" "), l1.join(","));
  const l2: string[] = [];
  const g2 = returnOut(l2);
  console.log(JSON.stringify(g2.next()), JSON.stringify(g2.next()), JSON.stringify(g2.next()), l2.join(","));
  const l3: string[] = [];
  const got: number[] = [];
  try {
    for (const v of throwOut(l3)) got.push(v);
  } catch (e) {
    console.log("caught", (e as Error).message, got.join(" "), l3.join(","));
  }
  const g4 = overArray([5, 6, 7]);
  console.log(g4.next().value, JSON.stringify(g4.return(0)), JSON.stringify(g4.next()));

  console.log("sum", await sumAsync([1, 2, 3, 4]));
  const l5: string[] = [];
  console.log("break", await breakAsync(l5), l5.join(","));
  console.log("await", await awaitLoop());
  const got2: number[] = [];
  for await (const v of agen([1, 2, 3])) got2.push(v);
  console.log("agen", got2.join(" "));
}

main();
