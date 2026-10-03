// A callback spelled through a binding reaches the closure that binding
// holds, even when several functions each bind the same name. Inputs
// are computed at run time (argv length is 2 on both sides).

const half = process.argv.length / 4;

function mapped(xs: number[]): number[] {
  const cb = (v: number): number => v + 1;
  return xs.map(cb);
}

function called(ys: number[]): number {
  const cb = (v: number): number => v * 3;
  let s = 0;
  ys.forEach((y: number): void => {
    s = s + cb(y);
  });
  return s;
}

function logged(zs: number[]): void {
  const cb = (v: number): void => {
    console.log(v);
  };
  zs.forEach(cb);
}

function aliased(ws: number[]): number[] {
  const cb = (v: number): number => v * 2;
  const again = cb;
  return ws.map(again);
}

function seeded(n: number): string {
  const zs: number[] = [n];
  const cb = (acc: number, x: number): number => acc + x;
  return String([1].reduce(cb, zs[n + 5]));
}

function chained(): void {
  const cb = (v: number): number => v + 1;
  Promise.resolve(half)
    .then(cb)
    .then((r: number): void => {
      console.log(r);
    });
}

console.log(mapped([half, 1, 2]).join(","));
console.log(called([half, 1]));
logged([half, 2]);
console.log(aliased([half, 3]).join(","));
console.log(seeded(process.argv.length));
chained();
