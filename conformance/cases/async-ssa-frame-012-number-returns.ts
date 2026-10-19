// An async function returning a number settles its promise with it at
// the width it computed it; every reader gets the number back, whether
// it awaits it, chains a `then` on it or collects it with others.

async function int(n: number): Promise<number> {
  await null;
  return n * 3;
}

async function half(n: number): Promise<number> {
  await null;
  return n / 2;
}

async function mixed(n: number): Promise<number> {
  await null;
  if (n > 2) {
    return n + 0.25;
  }
  return n;
}

async function chained(n: number): Promise<number> {
  const a = await int(n);
  const b = await half(a);
  return a + b;
}

async function main(): Promise<void> {
  console.log(await int(4), await half(5), await mixed(1), await mixed(3));
  console.log(await chained(7));
  const all = await Promise.all([int(1), half(3), mixed(4)]);
  console.log(all[0], all[1], all[2]);
  const v: any = await half(9);
  console.log("any", v);
}

main();
int(10).then((v) => console.log("then", v));
half(7).then((v) => console.log("then", v + 1));
