// A constructor on the ctor argv lane (some `super(...xs)` site
// spreads into it) with declared parameters. The §10.2.1.4
// short-call prologue — a parameter at or past the real argc binds
// undefined — counted the ctor's `__new_target` head slot as user
// position 0, so every declared parameter sat one position too far:
// the LAST one read undefined on every call, direct or spread, and
// `super()` with no argument cleared new.target itself.
class P3 { constructor(a, b, c) { console.log("P3", a, b, c, arguments.length); } }
class DS extends P3 { constructor(arr) { super(...arr); } }
class DD extends P3 { constructor() { super(4, 5, 6); } }
new P3(7, 8, 9);
new DD();
new DS([1, 2, 3]);
new DS([1]);
class Q1 { constructor(a) { console.log("Q1", a, new.target === Q1); } }
class DQ extends Q1 { constructor(arr) { super(...arr); } }
new Q1("only");
new DQ(["x"]);
class Z0 { constructor() { console.log("Z0", new.target === Z0, arguments.length); } }
class DZ extends Z0 { constructor(arr) { super(...arr); } }
class DZ0 extends Z0 { constructor() { super(); } }
new Z0();
new DZ([]);
new DZ0();
class T2 { constructor(a: number, b) { console.log("T2", a, b); } }
class DT extends T2 { constructor(arr) { super(...arr); } }
new DT([1, "two"]);
new T2(3, "four");
