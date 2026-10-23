// An async function waiting inside a `finally` body, with a return or
// a throw pending, on a promise only it can reach: the frame and the
// promise form a cycle nothing outside reaches, and the value the
// `finally` keeps aside belongs to it too (it holds the promise). Many
// such frames in a loop leave nothing behind, and neither the
// `finally` body's rest nor any handler of their promises runs.

class Pad {
  constructor(
    public p: Promise<number>,
    public xs: number[],
  ) {}
}

let after = 0;
let settled = 0;

function never(): Promise<number> {
  return new Promise<number>(() => {});
}

async function withReturn(i: number): Promise<Pad> {
  const p = never();
  try {
    return new Pad(p, new Array<number>(64).fill(i));
  } finally {
    await p;
    after++;
  }
}

async function withThrow(i: number): Promise<number> {
  const p = never();
  try {
    throw new Pad(p, new Array<number>(64).fill(i));
  } finally {
    await p;
    after++;
  }
}

async function main(): Promise<void> {
  for (let i = 0; i < 20000; i++) {
    withReturn(i).then(() => settled++);
    withThrow(i).catch(() => settled++);
  }
  const v = await Promise.resolve(7);
  console.log("after", after, "settled", settled, v);
}

main();
