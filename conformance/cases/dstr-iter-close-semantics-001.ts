// §7.4.6 IteratorClose completion semantics at a destructuring
// pattern's early stop: return-method-null skips the close,
// a non-Object answer is a TypeError, a throwing return propagates,
// and an object answer closes silently.
function mk(ret: any): any {
  const iterator: any = { next() { return { done: false, value: 1 }; } };
  if (ret !== "absent") iterator.return = ret;
  const iterable: any = {};
  iterable[Symbol.iterator] = function() { return iterator; };
  return iterable;
}
function probe(label: string, iterable: any) {
  let x: any;
  try {
    [x] = iterable;
    console.log(label, "ok", x);
  } catch (e: any) {
    console.log(label, "threw", e.constructor.name);
  }
}
probe("absent", mk("absent"));
probe("null-method", mk(null));
probe("undefined-method", mk(undefined));
probe("returns-object", mk(function() { return {}; }));
probe("returns-null", mk(function() { return null; }));
probe("returns-number", mk(function() { return 42; }));
probe("throws", mk(function() { throw new RangeError("boom"); }));

// abrupt completion through a for-of body: §7.4.6 step 7 — the
// original throw wins over anything the close raises, including the
// step-9 non-Object TypeError.
const it8: any = {
  next() { return { done: false, value: 1 }; },
  return() { return 42; }
};
const ib8: any = {};
ib8[Symbol.iterator] = function() { return it8; };
try {
  for (const v of ib8) { throw new RangeError("orig"); }
} catch (e: any) {
  console.log("abrupt", e.constructor.name, e.message);
}
