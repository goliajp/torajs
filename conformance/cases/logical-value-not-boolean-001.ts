// §13.13 — `a && b` and `a || b` evaluate to ONE OF THEIR OPERANDS,
// never to a boolean. The implicit return-annotation inference claimed
// `boolean` for both, so an unannotated function returning one either
// coerced its answer (`o.q && x` → `false` where node says `undefined`)
// or was rejected outright ("function expects Boolean, got String").
function andAny(a: any, b: any) { return a && b; }
console.log(andAny(0, "x"), andAny("y", "z"), andAny(undefined, 1), andAny(null, 1));

function orAny(a: any, b: any) { return a || b; }
console.log(orAny(0, "x"), orAny("y", "z"), orAny(undefined, 1));

function orStr(a: string, b: string) { return a || b; }
console.log(orStr("", "d"), orStr("k", "d"));

function andNum(n: number) { return n && n + 1; }
console.log(andNum(0), andNum(3));

function missAnd(o) { return o.q && 1; }
console.log(missAnd({ x: 9 }), missAnd({ q: 5 }));

function missAndEq(o: any) { return o.q && (o.r === undefined); }
console.log(missAndEq({ x: 9 }));

// Both operands boolean still names boolean — the shape the old
// blanket answer was right about.
function bothBool(a: number, b: number) { return a > 1 && b > 2; }
console.log(bothBool(0, 5), bothBool(2, 5));

function negated(a: any) { return !!(a && 1); }
console.log(negated(0), negated(2));
