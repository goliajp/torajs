// A `for (let i …)` head whose closures are all made in the body
// (§14.7.4.4). Each iteration's closures share that iteration's
// binding: a write through one is seen by the body and carried into the
// next copy, while a write made after the copy (by a body closure the
// test or the update calls) reaches only the old iteration's binding.
function bodyWrites(): string {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 9; i++) {
    const bump = () => (i += 2);
    fs.push(() => i);
    bump();
  }
  return fs.map((h) => h()).join(",");
}

function calledByTest(): string {
  const out: number[] = [];
  let g = () => 0;
  for (let i = 0; g() >= 0 && i < 3; i++) {
    g = () => (i += 100);
    out.push(i);
  }
  return out.join(",") + " " + g();
}

function calledByUpdate(): string {
  const out: number[] = [];
  let g = () => 0;
  for (let i = 0; i < 3; g(), i++) {
    g = () => (i += 100);
    out.push(i);
  }
  return out.join(",") + " " + g();
}

function continued(): string {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 6; i++) {
    fs.push(() => i);
    if (i % 2 === 0) continue;
    i++;
  }
  return fs.map((h) => h()).join(",");
}

function floatHead(): string {
  const fs: (() => number)[] = [];
  for (let x = 0.5; x < 3; x += 1) fs.push(() => x * 2);
  return fs.map((h) => h()).join(",");
}

function twoBindings(): string {
  const fs: (() => string)[] = [];
  for (let i = 0, j = 10; i < 3; fs.push(() => "u" + j), i++, j--) fs.push(() => i + ":" + j);
  return fs.map((h) => h()).join(",");
}

console.log("bodyWrites", bodyWrites());
console.log("calledByTest", calledByTest());
console.log("calledByUpdate", calledByUpdate());
console.log("continued", continued());
console.log("floatHead", floatHead());
console.log("twoBindings", twoBindings());
