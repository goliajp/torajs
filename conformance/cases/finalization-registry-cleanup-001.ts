// §9.13 / §26.2 — cleanup jobs. tr reclaims deterministically, so a
// target that loses its last reference is collected at once and its
// callback runs at the program's exit checkpoint, after microtasks.
// bun (a tracing collector) runs none of these without a forced gc,
// which the spec equally allows; the expected output is tr's.
let again = true;
const fr: FinalizationRegistry<unknown> = new FinalizationRegistry((held: any) => {
  console.log("cleanup", held);
  if (again) {
    again = false;
    fr.register({}, "registered by a callback");
    Promise.resolve().then(() => console.log("microtask from a callback"));
  }
});

function scope() {
  fr.register({}, "a");
  fr.register({ n: 1 }, { label: "b" });
}
scope();

const kept = {};
fr.register(kept, "kept alive to the end");

const token = {};
fr.register({}, "unregistered while pending", token);
console.log("unregister", fr.unregister(token));

// A registry reachable only from the cleanup queue still runs.
(function () {
  const lonely = new FinalizationRegistry((h: any) => console.log("lonely", h));
  lonely.register({}, 42);
})();

Promise.resolve().then(() => console.log("microtask"));
console.log("sync end");
