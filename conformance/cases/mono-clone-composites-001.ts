// monomorphized bodies deep-clone their expression graph; these
// composite variants used to fall through clone_leaf and panic
// (Delete / Elision — ssa_lower_deep_clone).
function del(m) {
  delete m["ba"];
}
var o: any = { aa: 1, ba: 2 };
del(o);
console.log(o.ba);

function mk(x) {
  return [x, , 3];
}
var a = mk(1);
console.log(a.length, a[0], a[2]);
