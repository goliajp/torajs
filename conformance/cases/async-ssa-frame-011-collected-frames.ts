// Frames parked on promises that only they keep alive form cycles
// with them (the promise's callback list holds the frame, the frame
// holds the promise, its parameters and its locals). Many of them are
// collected while others, whose promises stay reachable, wait and
// later finish with everything they held intact.

class Box {
  s: string;
  o: Box | null = null;
  constructor(s: string) {
    this.s = s;
  }
}

async function abandoned(i: number, q: Box): Promise<number> {
  const p = new Promise<number>(() => {});
  const s = "x" + i;
  const b = new Box(s);
  b.o = q;
  const v = await p;
  return v + b.s.length + q.s.length;
}

async function abandonedParam(p: Promise<number>, i: number): Promise<number> {
  const v = await p;
  return v + i;
}

const keep: any[] = [];
let sum = 0;

async function kept(i: number, q: Box): Promise<void> {
  const p = new Promise<number>((r) => {
    keep.push(r);
  });
  const s = "k" + i;
  const local = new Box(s);
  local.o = q;
  const v = await p;
  sum = sum + v + s.length + local.s.length + (local.o as Box).s.length;
}

for (let i = 0; i < 20000; i++) {
  abandoned(i, new Box("q" + i));
  abandonedParam(new Promise<number>(() => {}), i);
  if (i % 4 === 0) {
    kept(i, new Box("b" + i));
  }
}
for (const r of keep) {
  r(1);
}
Promise.resolve(0).then(() => console.log("sum", sum, keep.length));
