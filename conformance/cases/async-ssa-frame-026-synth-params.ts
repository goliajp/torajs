// An async function whose parameter the parser synthesized (an object or
// array pattern's source) suspends over its frame like any other: with a
// `for await`, with `arguments`, inside an arrow.
const log: string[] = [];
function ticks(tag: string, n: number) {
  let p = Promise.resolve();
  for (let i = 1; i <= n; i++) {
    p = p.then(() => {
      log.push(tag + i);
    });
  }
  return p;
}

async function objPattern({ a, b }: { a: number; b: number }) {
  let s = 0;
  for await (const x of [Promise.resolve(a), b]) {
    log.push("o" + x);
    s += x;
  }
  return s;
}

async function arrPattern([x, y]: number[], z: number) {
  log.push("argc " + arguments.length);
  const v = await Promise.resolve(x);
  log.push("arr after");
  return v + y + z;
}

const viaArrow = (k: number) => {
  async function g({ a }: { a: number }): Promise<number> {
    const v = await Promise.resolve(a);
    log.push("g after");
    return v + k;
  }
  return g({ a: 2 });
};

async function main() {
  ticks("A", 8);
  log.push("obj " + (await objPattern({ a: 1, b: 2 })));
  ticks("B", 6);
  log.push("arr " + (await arrPattern([1, 2], 3)));
  ticks("C", 6);
  log.push("arrow " + (await viaArrow(1)));
  await ticks("Z", 1);
  console.log(log.join(" "));
}
main();
