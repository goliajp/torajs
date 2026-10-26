// An async generator's `for` init of several declarations (a
// destructuring one is several too) runs once, in order, and suspends at
// each await in it: a later initializer reads an earlier binding, a
// closure an initializer made sees the value the loop started from, and
// the loop's bindings are copied per iteration as usual.

const log: string[] = [];
const P = (v: any) => Promise.resolve(v);
function ticks(n: number) {
  let p: Promise<void> = Promise.resolve();
  for (let i = 0; i < n; i++) p = p.then(() => { log.push("t" + i) });
}

async function* several() {
  for (let a = (log.push("a"), 1), b = await P(a + 1); a < 3; a++) {
    log.push("ab" + a + b);
    yield a + b;
  }
}

async function* pattern() {
  for (const { x, y } = await P({ x: 1, y: 2 }); ; ) {
    log.push("xy" + x + y);
    yield x + y;
    break;
  }
  for (let [p, q] = [await P(3), 4], fs: (() => number)[] = []; p < 5; p++) {
    fs.push(() => p);
    log.push("pq" + p + q + fs.length);
    yield p * q;
  }
}

async function* labeled() {
  outer: for (let i = 0, j = await P(10); i < 3; i++) {
    for (let k = i, f = () => k + j, m = await P(f()); k < 2; k++) {
      if (i === 1) continue outer;
      log.push(`i${i}k${k}m${m}f${f()}`);
      yield k;
    }
  }
  for (var a = 1, b = await P(a + 1); a < 2; a++) {
    log.push("var" + a + b);
    yield b;
  }
  const fs: (() => number)[] = [];
  for (let x = 0, y = await P(5); x < 2; x++) {
    fs.push(() => x + y);
    yield x;
  }
  log.push("fs" + fs.map((f) => f()).join(","));
}

// Every name the init declares is its own from the first initializer
// on: a closure reads a later one (not the outer `g`) once called, a
// direct read of a later one, or a call of such a closure before the
// later one is initialized, is a ReferenceError (§14.7.4.2 step 1 makes
// the bindings, uninitialized, before any initializer runs), and a
// `yield` in an initializer resumes with what `next` was handed.
const g = () => "outer";
async function* forward() {
  for (let f = () => g(), g = await P(() => "inner"); ; ) {
    log.push(f());
    yield 1;
    break;
  }
  try {
    for (let a = b, b = await P(2); ; ) {
      log.push("read" + a);
      break;
    }
  } catch (e: any) {
    log.push("tdz" + (e instanceof ReferenceError));
  }
  try {
    for (let h = () => w, u = h(), w = await P(3); ; ) {
      log.push("early" + u);
      break;
    }
  } catch (e: any) {
    log.push("ctdz" + (e instanceof ReferenceError));
  }
  for (let c = (yield "y"), d = await P(c + 1); ; ) {
    log.push("ya" + c + d);
    break;
  }
}

async function main() {
  ticks(3);
  const it = forward();
  for (let r = await it.next(); !r.done; r = await it.next(40)) log.push("n" + r.value);
  log.push("|");
  for (const g of [several, pattern, labeled]) {
    ticks(4);
    for await (const v of g()) log.push("v" + v);
    log.push("|");
  }
  console.log(log.join(" "));
}
main();
