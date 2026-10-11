// Same-named declarations of different kinds keep their kinds and answer
// `.name` with the name they were written with, both when the nested
// function captures an enclosing local and when it does not.

function c1(k: number) {
  function job() {
    return k;
  }
  return job;
}
function c2(k: number) {
  async function job() {
    return k;
  }
  return job;
}
function c3() {
  async function* job() {
    yield 1;
  }
  return job;
}
function c4() {
  function* job() {
    yield 2;
  }
  return job;
}
const fns = [c1(1), c2(2), c3(), c4()];
console.log(c1(0).name, c2(0).name);
console.log(fns.map((f) => typeof f).join(","));
console.log(c1(5)(), c4()().next().value);

// no capture: the nested function is lifted to the top level
function u1() {
  function solo() {
    return "sync";
  }
  return solo;
}
function u2() {
  async function solo() {
    return "async";
  }
  return solo;
}
console.log(u1()(), u1().name, u2().name);

// a block-level declaration of the shared name keeps working
function w1(flag: boolean) {
  if (flag) {
    const job = () => "block";
    return job();
  }
  return "none";
}
console.log(w1(true), w1(false));

async function main() {
  console.log(await c2(3)());
  for await (const v of c3()()) console.log("c3", v);
  console.log(await u2()());
}
main();
