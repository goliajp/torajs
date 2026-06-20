// ES §20.1.2.23 — Object.values(obj) + trailing-arg ignore.
// tora prior reject "no member `.values` on type Object("Object")"
// (SSA-emit had Obj/Arr/Str/Any dispatch, check.rs sig missing).
// S258 adds 1-arg sig + S256-extended trailing-arg ignore.

const o = { a: 10, b: 20, c: 30 };

// 1-arg base — homogeneous struct, Number values.
console.log(Object.values(o).length);
console.log(Object.values(o)[0]);
console.log(Object.values(o)[1]);
console.log(Object.values(o)[2]);

// trailing-arg silent drop (1 + 1 + N trailing).
console.log(Object.values(o, "extra").length);
console.log(Object.values(o, 99, "ignored").length);
console.log(Object.values(o, 1, 2, 3, 4, 5).length);

// String receiver — bun returns per-char Str array.
console.log(Object.values("ab").length);
console.log(Object.values("ab")[0]);
console.log(Object.values("ab")[1]);

// String receiver + trailing.
console.log(Object.values("xyz", "extra").length);
