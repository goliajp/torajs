// §27.6.1.2-4 — `AsyncGenerator.prototype.{next,return,throw}` route a
// bad receiver through AsyncGeneratorEnqueue step 3, which REJECTS the
// capability's promise with a TypeError. It does NOT throw: the call
// itself returns normally, so synchronous code after it keeps running
// and the failure surfaces on the microtask turn.
//
// This is the half of the detached step face that has a spec answer.
// The other half — a detached call whose receiver IS a live async
// generator — needs receiver-generic re-dispatch and still raises a
// loud TypeError; it is deliberately not exercised here, since tr and
// the reference engine disagree on it by construction.
//
// The receiver set mirrors test262's `this-val-not-object` (primitives
// and symbols) and `this-val-not-async-generator` (plain object,
// function, the generator function itself, its `.prototype`, and a
// SYNC generator instance — a different prototype root). `.prototype`
// is the sharp one: it sits directly under %AsyncGeneratorPrototype%,
// one hop closer than any instance, and must still reject.

async function* g() {
  yield 1;
}
const AGP: any = (Object.getPrototypeOf(g) as any).prototype;

function* sg() {
  yield 1;
}
const syncIt: any = sg();

const receivers: any[][] = [
  ["undefined", undefined],
  ["number", 1],
  ["string", "s"],
  ["null", null],
  ["boolean", true],
  ["plain object", {}],
  ["function", function () {}],
  ["async generator fn", g],
  ["async generator fn .prototype", (g as any).prototype],
  ["sync generator instance", syncIt],
];

const pending: any[] = [];
for (const entry of receivers) {
  const label: any = entry[0];
  for (const method of ["next", "return", "throw"]) {
    const p: any = AGP[method].call(entry[1]);
    pending.push(
      p.then(
        function () {
          console.log("FAIL " + method + " " + label + ": fulfilled");
        },
        function (e: any) {
          if (!(e instanceof TypeError)) {
            console.log("FAIL " + method + " " + label + ": " + e);
          }
        },
      ),
    );
  }
}

// The calls above all returned without throwing — this line proves the
// rejection is asynchronous rather than a swallowed synchronous error.
console.log("30 calls returned, none threw");

Promise.all(pending).then(function () {
  console.log("all 30 rejected with TypeError");
});
