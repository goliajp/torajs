// A `this`-reading declaration a relay reaches — `xs.map(t, o)` mints
// `__fwdrecv_t`, `(t as any)(…)` mints `__forward_t` — must not be
// folded by the static argv face off its one direct site: the relay's
// calls are runtime calls the fold cannot see. Before, `t.call(o, 1,
// 2, 3, 4)` as the only direct site folded `t` to 4, and through the
// callback `arguments.length` answered 4 where bun says 3 and
// `arguments[1]` answered undefined (rotation 650). The static face
// yields such a body to the this-first runtime face, as it yields a
// head-less one.
const o = { k: 1 };
const xs: any = [7];
function t(this: any, a: number): any { return [arguments[0], arguments[1], arguments[2] === undefined, typeof arguments[2], arguments.length, typeof this]; }
console.log("map", JSON.stringify(xs.map(t, o)));
console.log("anycall", JSON.stringify((t as any)(1, 2, 3, 4)));
console.log("call", JSON.stringify(t.call(o, 1, 2, 3, 4)));
function v(this: any, a: number): any { return [arguments.length, arguments[1], (this as any).k]; }
console.log(JSON.stringify(xs.map(v, o)), JSON.stringify(v.call(o, 1, 2, 3)));
function n(this: any, a: number): number { return arguments.length; }
console.log(JSON.stringify(xs.map(n, o)), n.call(o, 1, 2, 3, 4, 5));
