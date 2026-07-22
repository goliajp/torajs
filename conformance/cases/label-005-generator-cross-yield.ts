// generator with labeled break/continue crossing yield boundary
function* g() {
  outer: for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      if (j === 2) continue outer;
      yield i * 10 + j;
    }
  }
}
let a = "";
for (const x of g()) a += x + ",";
console.log(a);

function* h() {
  loop: for (let i = 0; i < 4; i++) {
    yield i;
    if (i === 2) break loop;
  }
  yield 99;
}
let b = "";
for (const y of h()) b += y + ",";
console.log(b);
