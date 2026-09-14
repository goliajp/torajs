// A receiver-first relay (`__fwdrecv_<t>`, the shell a this-reading
// declaration gets at a thisArg callback position) behaves like the
// plain relay on the routes the plain one has: its boxed adapter
// redirects into `t` with argv[0] feeding `__this` and the true argc
// / argv behind it, its forwarding call does not vote in the static
// argv fold, and it forwards its hidden argc. Before, all three read
// the `__forward_` prefix only, so through the any lane `arguments`
// answered the declared arity (rotation 649). A typed HOF site that
// reaches a this-first ARGV-face target (one that reads argument
// values) through either relay still answers the declared count — the
// relay's forwarding call packs its declared slots and the body
// materializes off that buffer — so the value-reading named-fn lines
// here go through the any lane; a length-only body is on the argc
// tier alone since rotation 650 and answers the true count on the
// typed route too (`t2`). The function-expression lines cover the
// typed argv-face route (which used to double-seed the receiver).
const o = { k: 1 };
function t3(this: any, a: number): any { return [arguments.length, a, arguments[2] === undefined ? "u" : "arr", (this as any).k]; }
const xs: any = [7];
console.log(JSON.stringify(xs.map(t3, o)));
console.log(JSON.stringify(xs.filter(t3, o)));
function t2(this: any, a: number): any { return [arguments.length, a, (this as any).k]; }
console.log(JSON.stringify([7].map(t2, o)), JSON.stringify(xs.map(t2, o)));
console.log(JSON.stringify([7, 8].filter(function (this: any, a: number): boolean { return arguments.length === 3 && (this as any).k === 1; }, o)));
[8].forEach(function (this: any, a: number): void { console.log(arguments.length, arguments[1], (this as any).k); }, o);
[9].map(function (this: any, a: number): number { console.log(JSON.stringify(this), a, arguments.length, arguments[1]); return a; }, o);
function cnt(this: any): number { return arguments.length; }
console.log([1, 2].every(cnt, o), [5].some(cnt, o));
