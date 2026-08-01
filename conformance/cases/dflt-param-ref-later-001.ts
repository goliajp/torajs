// §8.6.2 + §10.2.1.4: a later required param left unsupplied binds
// undefined; an earlier default reading it throws ReferenceError
// only when the initializer actually runs
function f(x = y, y) { return y; }
try { f(); } catch (e) { console.log((e as Error).name); }
console.log(f(7));
console.log(f(7, 8));

// return the earlier param: supplying it skips the initializer
function g(x = y, y) { return x; }
console.log(g(3));

// generator factory takes the same padding path
function* h(x = y, y) { yield y; }
try { h(); } catch (e) { console.log("gen", (e as Error).name); }
const it = h(1);
const s = it.next();
console.log(s.value, s.done);

// arrow shape
const a = (x = y, y) => y;
try { a(); } catch (e) { console.log("arrow", (e as Error).name); }
console.log(a(5));
