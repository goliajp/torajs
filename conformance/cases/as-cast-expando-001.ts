// r293 — `as` layers on an expando receiver peel: `(box as any).k`
// writes the same binding `box.k` does, so the degrade pass marks
// the declaration and the store rides the dynobj lane (the
// "struct StructId(N) has no field — layout: []" family).
function zig(): string {
  return "ziggy";
}
var box = {};
(box as any).ziggy = zig;
(box as any).n = 7;
console.log(typeof (box as any).ziggy, (box as any).ziggy(), (box as any).n);

// index spelling and delete ride the same peel
var box2 = {};
(box2 as any)["k"] = 1;
console.log((box2 as any)["k"]);
delete (box2 as any)["k"];
console.log((box2 as any)["k"]);
