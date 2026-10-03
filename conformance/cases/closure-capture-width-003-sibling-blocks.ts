// Same-named consts in sibling blocks of one function share a slot
// key; a callback read through either must still reach its own
// closure. One of each pair answers a fraction. Inputs are computed at
// run time (argv length is 2 on both sides).

const two = process.argv.length;
const half = two / 4;

function viaMap(xs: number[]): void {
  if (two > 1) {
    const cb = (v: number): number => v + half;
    console.log(xs.map(cb).join(","));
  }
  {
    const cb = (v: number): number => v * 2;
    console.log(xs.map(cb).join(","));
  }
}

function viaThen(): void {
  {
    const cb = (v: number): number => v / 4;
    Promise.resolve(two + 1).then(cb).then((r: number): void => {
      console.log("then a", r);
    });
  }
  {
    const cb = (v: number): number => v + 1;
    Promise.resolve(two).then(cb).then((r: number): void => {
      console.log("then b", r);
    });
  }
}

function viaCall(n: number): number {
  let s = 0;
  {
    const cb = (v: number): number => v + 1;
    s = s + cb(n);
  }
  {
    const cb = (v: number): number => v * half;
    s = s + cb(n + 1);
  }
  return s;
}

viaMap([1, two, 3]);
viaThen();
console.log(viaCall(two));
