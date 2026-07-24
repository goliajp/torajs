// dynobj-degrade expando triggers (rotation 203 chunk 2) — a
// dynamic-member write / computed-key write / member delete on an
// unannotated ObjectLit binding degrades the declaration to the
// dynobj lane, giving the test262-pervasive "expando on a plain
// object" idiom (`var obj = {}; obj.length = 0.1`) a legal lane.
// Own-field writes on never-triggered bindings stay on the static
// struct lane (last face).

var obj = {};
obj.length = 0.1;
console.log(obj.length);
obj.name = "n";
console.log(obj.name);

let b = { x: 1 };
b.y = 2;
console.log(b.x);
console.log(b.y);

let c = { x: 1 };
delete c.x;
console.log(c.x);

let d = {};
let k = "dyn";
d[k] = 7;
console.log(d[k]);

let e = { v: 5 };
e.v = 6;
console.log(e.v);
