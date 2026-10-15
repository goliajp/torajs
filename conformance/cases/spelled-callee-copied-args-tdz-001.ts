// A call through `(c ? f : g)(args)` copies its arguments into the
// second branch. A read the arguments make of a for-of head's own
// binding, or of a class from its computed key, is in the binding's
// temporal dead zone (§14.7.5.6, §15.7.14) whichever branch runs.
function f(x: any) {
  return x;
}
function g(x: any) {
  return x;
}
function run(label: string, body: () => void) {
  try {
    body();
    console.log(label, "no throw");
  } catch (e: any) {
    console.log(label, e instanceof ReferenceError);
  }
}
let c = false;
run("for-of else", () => {
  let x: any = [1];
  for (let x of (c ? f : g)(x)) {
    console.log(x);
  }
});
c = true;
run("for-of then", () => {
  let x: any = [1];
  for (let x of (c ? f : g)(x)) {
    console.log(x);
  }
});
c = false;
run("class key else", () => {
  class C {
    static [(c ? f : g)(C)] = 1;
  }
  console.log(C);
});
c = true;
run("class key then", () => {
  class C {
    static [(c ? f : g)(C)] = 1;
  }
  console.log(C);
});
run("comma", () => {
  let x: any = [2];
  for (let x of (0, g)(x)) {
    console.log(x);
  }
});
