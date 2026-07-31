// Variable-routed fn-expr callbacks through the HOF / Array.from
// faces (rotation 260): every promoted binding now registers in
// fnexpr_recv_locals, which is how the HOF lowerings detect a
// promoted callback behind an Ident hop — a pure-face binding left
// the set empty, so the loop called the RECV ABI without the
// __this argv slot (`this` read the element box, the element read
// garbage). Also exercises the knife-2 define face and the
// mixed-profile direct call to pin their pre-existing behavior.
const mf = function (v: any) {
  return v + (this as any).k;
};
console.log(JSON.stringify(Array.from([1, 2], mf, { k: 10 })));
var acc: any[] = [];
const collector = function (v: any) {
  acc.push(v * (this as any).m);
};
[3, 4].forEach(collector, { m: 2 });
console.log(JSON.stringify(acc));
const zeroArity = function () {
  acc.push((this as any).z);
};
[1].forEach(zeroArity, { z: 42 });
console.log(JSON.stringify(acc));
const g = function () {
  return this === undefined ? "u" : (this as any)._v;
};
var p: any = { _v: 9 };
p.__defineGetter__("w", g);
console.log(p.w);
console.log(g());
