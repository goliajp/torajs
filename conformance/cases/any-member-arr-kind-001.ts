// A typed array stored through an any-receiver member write must be
// readable back.
//
// `o.x = [7, 8]` stored a block with no elem-kind chain — the
// refcounted arm of the dynobj-assign lane never stamped one — so
// every any-side reader answered empty: `String(o.x)` printed
// nothing, `o.x[0]` was undefined. Silent, one statement. An
// `any`-typed rhs was boxed (and stamped) upstream and always read
// back fine, which is what hid this: the failure needed the rhs to
// KEEP its static array type all the way to the store. The
// struct-field lane fixed the same omission in chunk 621.

var o: any = {};

o.x = [7, 8];
console.log(String(o.x), o.x[0], o.x.length);

// each element family carries a different chain
o.s = ["a", "b"];
console.log(String(o.s), o.s[1]);
o.f = [1.5, 2.5];
console.log(String(o.f), o.f[1]);
o.b = [true, false];
console.log(String(o.b), o.b[0]);

// nested arrays: the outer stamp is this fix, the inner ones ride
// the literal's own lowering
o.n = [[1], [2, 3]];
console.log(o.n[1][0], o.n[1].length);

// a typed variable rhs (not just a literal)
var a = [9, 8, 7];
o.v = a;
console.log(String(o.v));

// the store is a SHARE, not a copy — mutation through the source
// stays visible
a.push(6);
console.log(String(o.v), o.v.length);

// statement-position writes in a loop: the last one wins and reads
var last: any = {};
for (let i = 0; i < 3; i++) {
  last.loop = [i, i * 2];
}
console.log(String(last.loop));

// the shapes that always worked must not move
o.str = "hi";
o.num = 42;
o.obj = { a: 1 };
console.log(o.str, o.num, o.obj.a);
var av: any = [5, 5];
o.fromany = av;
console.log(String(o.fromany));
