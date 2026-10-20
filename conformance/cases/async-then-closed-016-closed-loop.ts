// A loop of awaits of a class instance in a program that gives no
// object a `then`: each await settles with the instance one job later,
// interleaved with another function's awaits.

class Acc {
  n = 0;
  add(k: number): number {
    this.n += k;
    return this.n;
  }
}

const log: string[] = [];

async function sum(count: number): Promise<number> {
  const acc = new Acc();
  for (let i = 0; i < count; i++) {
    const a = await acc;
    a.add(i);
    if (i < 3) log.push("s" + i);
  }
  return acc.n;
}

async function other() {
  for (let i = 0; i < 3; i++) {
    log.push("o" + i);
    await 0;
  }
}

sum(1000).then((v) => console.log(v, log.join(",")));
other();
