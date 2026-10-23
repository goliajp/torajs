// Values that cross an await inside a `try` or a `for…of` keep their
// value: one defined before the statement and used after it, one read
// by a call between its definition and the await, one defined right
// before the await in the same block, numbers, strings and objects in
// a loop whose awaits sit in a try, and a promise a binding holds that
// is awaited twice.

function later<T>(v: T): Promise<T> {
  return Promise.resolve(v);
}

class Point {
  constructor(
    public x: number,
    public y: number,
  ) {}
}

function touch(p: Point): number {
  return p.x + p.y;
}

async function beforeAndAfter(n: number): Promise<string> {
  const p = new Point(n, n + 1);
  const t = touch(p);
  const s = "s" + n;
  const f = n / 4;
  try {
    await later(0);
    if (n === 2) throw new Error("two");
  } catch (e) {
    await later(1);
  }
  return [p.x, p.y, t, s, f].join(",");
}

async function sameBlock(n: number): Promise<string> {
  const parts: string[] = [];
  for (const i of [1, 2, 3]) {
    try {
      const local = "v" + i * n;
      const obj = new Point(i, n);
      await later(i);
      parts.push(local + ":" + obj.x + obj.y);
    } finally {
      parts.push("f" + i);
    }
  }
  return parts.join(" ");
}

async function loopCarried(): Promise<string> {
  let acc = 0.5;
  let text = "";
  let last: Point | null = null;
  for (let i = 0; i < 5; i++) {
    try {
      acc += await later(i * 1.25);
      text += await later(String.fromCharCode(97 + i));
      last = new Point(i, i * i);
    } catch (e) {
      text += "!";
    }
  }
  return acc + " " + text + " " + (last ? last.y : -1);
}

async function awaitTwice(): Promise<string> {
  const p = later("twice");
  const out: string[] = [];
  try {
    out.push(await p);
    out.push(await p);
  } finally {
    out.push(await p);
  }
  for (const k of [p, p]) {
    out.push((await k).length + "");
  }
  return out.join(",");
}

async function main(): Promise<void> {
  for (let n = 1; n <= 3; n++) {
    console.log(await beforeAndAfter(n));
  }
  console.log(await sameBlock(7));
  console.log(await loopCarried());
  console.log(await awaitTwice());
}

main();
