// A parameter with no default sits in front of one that has it. The
// language binds undefined at the hole and the default behind it
// (§10.2.11); tr used to abandon the whole call-site pad at the hole
// and report an arity error instead.
class Inst { m(x, y = 5) { return "i:" + x + ":" + y } }
console.log(new Inst().m(), new Inst().m(1), new Inst().m(1, 2));

class Stat { static m(x, y = 5) { return "s:" + x + ":" + y } }
console.log(Stat.m(), Stat.m(1));

class Gen { *m(x, y = 5) { yield "g:" + x + ":" + y } }
console.log([...new Gen().m()].join(","));

// Two holes, then a default.
class Two { m(a, b, c = 3) { return "t:" + a + ":" + b + ":" + c } }
console.log(new Two().m(), new Two().m(1));

// A rest tail is not a paddable slot: what an absent variadic tail
// gets is an empty array, never an undefined pushed into it. `V.f`
// shares its name with an unrelated fixed-arity row, so the receiver
// is not resolvable and the by-name table is what answers.
class V { f(x = 9, ...r) { return "v:" + x + ":" + r.length } }
class W { f(p, q) { return "w:" + p + ":" + q } }
console.log(new V().f(), new V().f(1), new V().f(1, 2, 3));

// By-name soundness: a same-shape row that declares NO default must
// not be handed the other owner's. `B.m`'s y is undefined, not 5.
class DA { m(x, y = 5) { return "A:" + x + ":" + y } }
class DB { m(x, y) { return "B:" + x + ":" + y } }
const both: any[] = [new DA(), new DB()];
for (const o of both) console.log(o.m(), o.m(1));
