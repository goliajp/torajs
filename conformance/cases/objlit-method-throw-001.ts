// objlit struct-field call lanes must propagate a pending throw
var r: string[] = [];
var o1 = { m() { throw new Error("a"); } };
try { o1.m(); r.push("m:no"); } catch (e) { r.push("m:" + (e as Error).message); }
var o2 = { f: function () { throw 42; } };
try { o2.f(); r.push("f:no"); } catch (e) { r.push("f:" + String(e)); }
var o3 = { a: () => { throw new RangeError("c"); } };
try { o3.a(); r.push("a:no"); } catch (e) { r.push("a:" + (e instanceof RangeError)); }
var o4 = { n: 0, m() { if (this.n === 0) { throw new Error("zero"); } return this.n; } };
try { o4.m(); r.push("this:no"); } catch (e) { r.push("this:" + (e as Error).message); }
o4.n = 7;
r.push("ok:" + o4.m());
var o5 = { get g(): number { throw new Error("getter"); } };
try { var x = o5.g; r.push("g:no"); } catch (e) { r.push("g:" + (e as Error).message); }
var o6 = { t: 1, toJSON() { throw new Error("tojson"); } };
try { JSON.stringify(o6); r.push("j:no"); } catch (e) { r.push("j:" + (e as Error).message); }
console.log(r.join(" "));
