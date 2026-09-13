// 639-14 — ES §14.7.5: a property deleted during enumeration is not
// visited, even when the delete happens inside a helper that receives
// the object through an untyped parameter. The literal's binding has
// to live on the dynobj lane for the any-lane delete to remove
// anything (test262 S12.6.4_A7_T1).
var obj = { aa: 1, ba: 2, ca: 3 };
var accum = "";
for (var key in obj) {
  erasator(obj, "b");
  accum += key + obj[key];
}
console.log(accum);

// declared first, assigned later (the test262 spelling)
var late, acc2;
late = { aa: 1, ba: 2, ca: 3 };
acc2 = "";
for (var k2 in late) {
  erasator(late, "c");
  acc2 += k2 + late[k2];
}
console.log(acc2);

// forwarded through a second helper
var fwd = { pa: 1, qa: 2 };
function relay(tag, target) {
  erasator(target, tag);
}
relay("q", fwd);
console.log(Object.keys(fwd).join(","));

// a helper that only reads keeps the arg static
var still = { aa: 1, ba: 2 };
function peek(h) {
  return h.aa;
}
console.log(peek(still), Object.keys(still).join(","));

function erasator(hash_map, charactr) {
  for (var key in hash_map) {
    if (key.indexOf(charactr) === 0) {
      delete hash_map[key];
    }
  }
}
