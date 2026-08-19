// A user descendant of an exotic-parent class mints the ROOT's real
// builtin cell (rotation 451): `new CP2(executor)` is a live Promise
// cell riding the whole then/settle machinery, and the inherited
// capability settle constructs a CP2 instance through the two-hop
// ctor chain.
class CP extends Promise<any> {}
class CP2 extends CP {}
const p: any = new CP2((res: any) => res(5));
console.log(p instanceof CP2, p instanceof CP, p instanceof Promise);
p.then((v: any) => console.log("then", v));
const q = (CP2 as any).resolve("two-hop");
console.log(q instanceof CP2);
q.then((v: any) => console.log("resolved", v));
