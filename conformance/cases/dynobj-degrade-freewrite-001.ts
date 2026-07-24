// Rotation 205 — free-receiver write triggers: an expando write /
// computed-key write / member delete whose receiver resolves Free
// (fn-body use of a top-level binding) degrades the declaration
// through the name-keyed fallback, reaching the named-fn-visible
// Any-global lane. The own-field gate holds through the fallback.

// face 1 — the pervasive var-form named-fn expando idiom.
var obj = {};
function f() {
  obj.x = 1;
  return obj.x;
}
console.log(f());
console.log(obj.x);

// face 2 — let form, same trigger.
let cfg = {};
function setup() {
  cfg.mode = "fast";
}
setup();
console.log(cfg.mode);

// face 3 — free member delete.
var d = { a: 1, b: 2 };
function drop() {
  delete d.a;
}
drop();
console.log(d.a);
console.log(d.b);

// face 4 — free computed-key write.
var bag = { x: 1 };
function put() {
  bag["y"] = 9;
}
put();
console.log(bag.y);
console.log(bag.x);
