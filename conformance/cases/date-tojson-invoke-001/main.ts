// rotation 431 — §21.4.4.37 step 3 is Invoke(O, "toISOString"): a
// plain object's OWN toISOString wins over the builtin when
// Date.prototype.toJSON is .call-re-dispatched onto it.
const result = { tag: 1 };
const out = Date.prototype.toJSON.call({ toISOString: function () { return result; } });
console.log((out as any) === result);
const d = new Date(0);
console.log(d.toJSON());
console.log(JSON.stringify({ when: d }));
