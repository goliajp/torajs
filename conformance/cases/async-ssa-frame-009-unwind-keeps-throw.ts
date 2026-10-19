// A throw already on its way out of a function can release the last
// reference to a promise an async function waits on: that frame is
// dropped and unwound in the middle of the propagation, and the throw
// still reaches the caller's catch, with its own value.

function never(): Promise<number> {
  return new Promise<number>(() => {});
}

let holder: Promise<number> = Promise.resolve(0);

async function waitOn(): Promise<number> {
  const label = "waiting";
  const v = await holder;
  return v + label.length;
}

function g(n: number): void {
  const local = never();
  holder = local;
  waitOn();
  holder = Promise.resolve(1);
  throw new Error("boom " + n);
}

for (let i = 0; i < 3; i++) {
  try {
    g(i);
    console.log("no throw");
  } catch (e) {
    console.log("caught", (e as Error).message);
  }
}

class Problem {
  constructor(public code: number) {}
}

function h(): number {
  const local = never();
  holder = local;
  waitOn();
  holder = Promise.resolve(2);
  throw new Problem(7);
}

try {
  h();
} catch (e) {
  console.log("caught code", (e as Problem).code);
}
console.log("end");
