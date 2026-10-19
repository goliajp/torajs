// An async function suspends over a heap frame: what lives across an
// await (numbers, doubles, strings, objects, a value read by a call
// before the await and again after it) comes back intact, in loops as
// in straight-line code, and the same promise can be awaited twice.

class Box {
  constructor(public v: number) {}
}

function touch(s: string): number {
  return s.length;
}

async function three(n: number): Promise<number> {
  const a = n * 2;
  await null;
  const b = a + 1;
  await Promise.resolve(0);
  const c = b * 3;
  await 7;
  return a + b + c;
}

async function kinds(x: number): Promise<string> {
  const d = x / 4;
  const s = "s" + x;
  const o = new Box(x + 1);
  const used = touch(s);
  await null;
  const mid = d + o.v + used;
  await Promise.resolve("ignored");
  return s + ":" + d + ":" + o.v + ":" + used + ":" + mid;
}

async function loop(n: number): Promise<number> {
  let total = 0;
  const base = n + 1000;
  for (let i = 0; i < n; i++) {
    total += await Promise.resolve(i);
    if (i % 2 === 0) {
      await null;
    }
  }
  return total + base;
}

async function twice(): Promise<number> {
  const p = Promise.resolve(21);
  const x = await p;
  const y = await p;
  return x + y;
}

async function returnAwait(): Promise<string> {
  return await Promise.resolve("ra");
}

async function main(): Promise<void> {
  console.log(await three(5));
  console.log(await kinds(10));
  console.log(await loop(6));
  console.log(await twice());
  console.log(await returnAwait());
  const all = await Promise.all([three(1), three(2), loop(3)]);
  console.log(all.join(","));
}

main();
