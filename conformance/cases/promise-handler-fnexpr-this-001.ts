// §27.2.5.4 step 9 calls a then/catch/finally handler with NO
// receiver, so a function EXPRESSION handler sees `this === undefined`
// in strict code — the same answer a NAMED function handler already
// gave. Before rotation 386 the expression spelling refused to
// compile (`closure __closure_N references unknown identifier
// __this`), so the answer depended on how the callback was spelled.

function named(): void {
  console.log("named", typeof this);
}

Promise.resolve(1).then(function (v: any) {
  console.log("then", typeof this, v);
});

Promise.resolve(2).then(named);

Promise.reject(new Error("boom")).catch(function (e: any) {
  console.log("catch", typeof this, e.message);
});

Promise.resolve(3).finally(function () {
  console.log("finally", typeof this);
});

// Second handler slot of `then`, and a chain root that is itself a
// handler call (the certainty check recurses through it).
Promise.resolve(4)
  .then(function (v: any) {
    return v + 1;
  })
  .then(
    function (v: any) {
      console.log("chain", typeof this, v);
    },
    function () {
      console.log("unreached");
    },
  );

// A handler that also captures a real outer binding — the `__this`
// removal must leave the rest of the env layout intact.
const tag = "cap";
Promise.resolve(5).then(function (v: any) {
  console.log("env", typeof this, tag, v);
});
