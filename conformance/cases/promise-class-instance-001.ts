// rotation 234 — a class instance as a promise's settled value.
// `Promise.reject(new Error("boom"))` is the test262 async family's
// canonical rejection reason; the checker's v0.5 whitelist was the
// only gate (a ClassRef is a nominal heap struct at SSA, so the
// runtime adopt path and the receiver-generic then/catch station
// already handled it).

const p = Promise.reject(new Error("boom"));
p.catch((e) => {
  console.log("caught:", e.message);
});

// A user class rides the same shape on the resolve side.
class Foo {
  x: number = 7;
}
Promise.resolve(new Foo()).then((f) => {
  console.log("got:", f.x);
});

// The async/await form — reason surfaces through catch with its
// class face intact.
async function main() {
  try {
    await Promise.reject(new Error("async-boom"));
  } catch (e) {
    console.log("name:", e.name, "msg:", e.message);
    console.log("is-err:", e instanceof Error);
  }
}
main();

// Two-handler station over a native-error subclass instance.
const q = Promise.reject(new RangeError("deep"));
q.then(
  (v) => {
    console.log("nope", v);
  },
  (e) => {
    console.log("two-arg:", e.message);
  },
);
