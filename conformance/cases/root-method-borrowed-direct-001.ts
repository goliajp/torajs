// A root toString / valueOf read off a prototype and called on a
// foreign receiver — Object.prototype's two, Array.prototype.toString
// and Function.prototype.toString. Each answers its own kernel; the
// object identity of Object.prototype.valueOf's answer is the
// receiver itself (§20.1.3.7 ToObject of an object).
const ts: any = Object.prototype.toString;
const vo: any = Object.prototype.valueOf;
console.log(ts.call({}), ts.call([1]), ts.call(() => 1), ts.call(new Date(0)).slice(0, 8));
const o = { a: 1 };
console.log(vo.call(o) === o, vo.call(Number.prototype) === Number.prototype);
const xs = [1, 2];
console.log(vo.call(xs) === xs, (Array.prototype.toString as any).call(xs));
console.log((Array.prototype.toString as any).call({ join: () => "J" }));
function f(a: number) { return a; }
console.log(typeof (Function.prototype.toString as any).call(f));
console.log(ts.call({ toString() { return "own"; } }));
