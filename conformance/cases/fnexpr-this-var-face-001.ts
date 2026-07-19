// fn-expr `this` through a variable-routed accessor face (RFC
// 20260717-fnexpr-this-channel knife 2): a single-use const binding
// whose init is a function expression promotes to the receiver-first
// channel at its face position
const s = function (v: any) { this._y = v * 3; };
const o: any = {};
o.__defineSetter__("y", s);
o.y = 5;
console.log(o._y);

const g = function () { return this._v ?? "empty"; };
const p: any = {};
Object.defineProperty(p, "v", { get: g });
console.log(p.v);
p._v = "loaded";
console.log(p.v);

const s3 = function (x: any) { this.raw = x * 10; };
const q: any = {};
q.__defineSetter__("scaled", s3);
q.scaled = 4;
console.log(q.raw);

// a this-free fn-expr keeps the plain closure ABI
const both = function () { return 5; };
const r: any = {};
Object.defineProperty(r, "five", { get: both });
console.log(r.five);
