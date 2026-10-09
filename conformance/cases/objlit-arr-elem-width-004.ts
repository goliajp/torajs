// Element structs widen across function returns, fn-typed slots, a
// promise payload, a push loop and serialization.
type A = { right: number };
function mk(): A[] {
  return [{ right: 3.5 }];
}
const f: () => A[] = mk;
console.log(mk()[0].right, f()[0].right);

type P = { x: number; y: number };
const ps: P[] = [];
for (let i = 0; i < 4; i++) ps.push({ x: i, y: i / 2 });
let s = 0;
for (const p of ps) s += p.x + p.y;
console.log(s, ps[3].y, JSON.stringify(ps[1]), JSON.stringify(ps));

async function g(): Promise<A[]> {
  return [{ right: 0.25 }];
}
g().then((v) => console.log(v[0].right));
