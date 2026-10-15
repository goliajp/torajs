// A call through `(c ? f : g)(args)` runs the arguments in whichever
// branch the condition picks: an optional call among them still
// short-circuits, and a tagged template among them is still one site
// with one template object.
function f(x: any) {
  return "f:" + x;
}
function g(x: any) {
  return "g:" + x;
}
let o: any = null;
let c = false;
console.log((c ? f : g)(o?.m()));
console.log((c ? f : g)(o?.n?.m()));
o = { m: () => 7 };
console.log((c ? f : g)(o?.m()));
c = true;
console.log((c ? f : g)(o?.m()));

function tag(s: any) {
  return s;
}
const seen: any[] = [];
function keepA(s: any) {
  seen.push(s);
}
function keepB(s: any) {
  seen.push(s);
}
for (let i = 0; i < 4; i++) (i % 2 ? keepA : keepB)(tag`x${i}y`);
console.log(seen.length, seen[0] === seen[1], seen[1] === seen[2], seen[0].raw.join("|"));
