// A top-level literal whose member names another top-level array /
// object literal binding is still promoted for named-fn reads (it
// boxes like the `: any[]` spelling of the same literal did).
const c0 = [1];
const c1 = { x: c0 };
function h3() { return c1.x.length; }
const d0 = [1];
const d1 = [d0, d0];
function h4() { return d1.length + d1[1][0]; }
const e0 = { k: "v" };
const e1 = [e0, { e0 }];
const e2 = { list: e1 };
function h5() { return e2.list.length + ":" + e2.list[0].k; }
const a1 = [[1]];
function h1() { return a1.length; }
console.log(h3(), h4(), h5(), h1());
d0.push(2);
console.log(h4(), d1[0].length, JSON.stringify(e2));
