// The two close obligations the stepping lane must NOT change.
// A step that throws sets [[done]] — no close. A pattern that
// completes over a still-live iterator does owe one.
function run(mkNext: () => any, label: string): void {
  let nextCount = 0;
  let returnCount = 0;
  const iterator: any = {
    next: function () {
      nextCount += 1;
      return mkNext();
    },
    return: function () {
      returnCount += 1;
      return {};
    },
  };
  const iterable: any = {};
  iterable[Symbol.iterator] = function () {
    return iterator;
  };
  const o: any = {};
  let caught = "none";
  try {
    [o["k"]] = iterable;
  } catch (e: any) {
    caught = e.message;
  }
  console.log(label, nextCount, returnCount, caught, o.k);
}
run(function () {
  throw new Error("boom");
}, "thrw-skip");
run(function () {
  return { done: true, value: undefined };
}, "nrml-skip");
run(function () {
  return { done: false, value: 7 };
}, "nrml-close");
