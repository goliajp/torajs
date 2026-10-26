// An async generator's await in a loop head, a case value or a for-in
// body suspends where the spec evaluates it: a `for` init once, a step
// and a `do … while` test before every later iteration (after
// `continue` too), a case value only while no earlier case matched.

const log: string[] = [];
const P = (v: any) => Promise.resolve(v);

async function* forStep() {
  const fs: (() => number)[] = [];
  for (let i = await P(0); i < 4; i = await P(i + 1)) {
    if (i === 1) continue;
    fs.push(() => i);
    yield i;
  }
  // each iteration has its own `i`
  yield fs.map((f) => f()).join(",");
}

async function* forCond() {
  outer: for (let i = 0; await P(i < 3); i++) {
    for (let j = 0; j < 3; j++) {
      if (j === 1) continue outer;
      yield `${i}${j}`;
    }
  }
}

async function* forExprInit() {
  let i = 0;
  for (i = await P(5); i < 7; i++) yield i;
}

async function* doWhile() {
  let i = 0;
  lbl: do {
    i++;
    if (i === 2) continue lbl;
    if (i === 5) break;
    yield i;
  } while (await P(i < 9));
}

async function* sw(x: any) {
  switch (x) {
    case await P(1):
      yield "one";
    case 2:
      yield "two (or fell)";
      break;
    default:
      yield "default";
    case await P(3):
      yield "three (or fell)";
  }
}

let tests = 0;
async function* swOrder(x: any) {
  const probe = (v: number) => {
    tests++;
    return P(v);
  };
  switch (x) {
    case await probe(1):
      yield "first";
      break;
    case await probe(2):
      yield "second";
      break;
  }
  yield `tests ${tests}`;
}

async function* forIn(o: any) {
  for (const k in o) {
    const v = await P(o[k]);
    yield `${k}=${v}`;
  }
}

async function* misc(o: any) {
  let a: any, b: any;
  [a, b] = await P([1, 2]);
  yield a + b;
  o[await P("k")]++;
  yield o.k;
  delete o[await P("k")];
  yield JSON.stringify(o);
  const n: any = null;
  yield (await P(n))?.x;
}

async function main() {
  const gens = [
    forStep(),
    forCond(),
    forExprInit(),
    doWhile(),
    sw(1),
    sw(2),
    sw(3),
    sw(9),
    swOrder(1),
    swOrder(2),
    forIn({ a: 1, b: 2 }),
    misc({ k: 1 }),
  ];
  for (const g of gens) {
    for await (const v of g) log.push(String(v));
    log.push("|");
  }
  console.log(log.join(" "));
}

main();
