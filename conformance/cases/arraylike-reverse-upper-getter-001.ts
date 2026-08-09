// rotation 349 knife 5b — the array-like mutator walks stop on a
// pending throw (Has/Get/Set reach user accessors; the throw
// previously rode silently to the end of the walk). §23.1.3.26
// reverse starts at the UPPER end, so a getter at len-1 under a
// giant ToLength'd length must throw out of the FIRST round — this
// read as a hang before the check landed.
var arrayLike: any = { length: 2 ** 53 + 2 };
Object.defineProperty(arrayLike, "9007199254740990", {
  get: function () { throw new Error("stop-upper"); },
});
try {
  (Array.prototype.reverse as any).call(arrayLike);
  console.log("no-throw");
} catch (e) { console.log("threw:", (e as any).message); }
