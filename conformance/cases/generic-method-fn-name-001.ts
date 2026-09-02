// 562-09 — a generic class's method lowers only as its all-`any`
// mono instance (`check_monomorph_any_widen` appends `$$anywv`), and
// the fn-name registry took the symbol as written: the reified
// prototype entry printed `[Function: get2$$anywv]` and `.name`
// answered the same. The ES name is the source spelling, so the
// instance suffix comes off the row.
class G<T> { get2(x: T) { return x; } m2() {} }
const g = new G<number>();
console.log(g);
console.log(g.get2);
console.log(JSON.stringify(g.get2.name));
console.log(g.get2(7));
console.log(JSON.stringify(Object.getOwnPropertyNames(G.prototype)));

// A NON-generic class is the control — its symbol never carried a
// suffix and must keep answering the same name.
class P { p1() {} }
console.log(new P().p1);
console.log(JSON.stringify(new P().p1.name));

// A property key may itself contain `$$`; only a real instance
// suffix (`$$anywv`, `$$_<type>`) comes off.
class Q { "a$$b"() {} }
console.log(new Q());
console.log(JSON.stringify(Object.getOwnPropertyNames(Q.prototype)));

// A generic class with two type parameters and a method taking both.
class Pair<A, B> { both(a: A, b: B) { return [a, b]; } }
const pr = new Pair<number, string>();
console.log(pr.both);
console.log(JSON.stringify(pr.both.name));
console.log(pr.both(1, "x"));
