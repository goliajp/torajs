// A field written from a tainted parameter carries the sentinel out
// to whoever reads the field. The read is lowered in the caller,
// while the parameter belongs to the callee, so the lowering-side
// mirror of the field table could not see the connection on its own.
const zs: number[] = [1, 2, 3];

class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

const b = new Box(zs[9]);
console.log(typeof b.v, b.v === undefined);
console.log(typeof new Box(5).v, new Box(5).v);

function make(w: number): { w: number } {
  return { w: w };
}

const o = make(zs[9]);
console.log(typeof o.w, o.w === undefined);
console.log(make(7).w);
