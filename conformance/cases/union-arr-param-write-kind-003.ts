// a typed array handed to a slot whose array face holds more than one
// element kind takes values of the other kinds through that slot
const ar = (xs: (string | number)[]) => { xs.push("a"); return xs.length };
class K { m(xs: (string | boolean)[] | any[]) { xs.push(1); return xs.length } }
function local() {
  const v: number[] = [1, 2];
  const w: boolean[] = [false];
  const r1 = ar(v);
  const r2 = new K().m(w);
  let t = 0;
  for (let i = 0; i < 2; i++) t += v[i];
  console.log(r1, r2, t, v, w);
}
local();
type P = { x: number };
function pts(ps: (P | string)[]) { ps.push("p"); return ps.length }
const q: P[] = [{ x: 1 }];
console.log(pts(q), q[0].x, q[1]);
