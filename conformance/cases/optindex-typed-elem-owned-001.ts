// An optional-chain index read of a typed array's heap element, bound
// or discarded inside a loop, leaves the array's own reference alone.
// The read used to answer the element without taking a reference while
// its consumer released one, so each iteration freed the element once:
// the array then printed another object's contents.
const n = 7;
const strs = ["x" + String(n), "y" + String(n)];
const objs = [{ t: "o" + String(n) }];

function bind(): void {
  for (let i = 0; i < 3; i++) {
    const a = strs?.[1];
    const b = objs?.[0];
    console.log(a, b === objs[0]);
  }
}
function discard(): void {
  for (let i = 0; i < 3; i++) {
    strs?.[0];
    objs?.[0];
  }
}
function pass(): string {
  let out = "";
  for (let i = 0; i < 3; i++) {
    out += String(strs?.[i % 2]);
  }
  return out;
}

bind();
discard();
console.log(pass());
console.log(strs.join(","), JSON.stringify(objs));
