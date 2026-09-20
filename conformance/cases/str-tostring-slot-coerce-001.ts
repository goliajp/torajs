// ES §7.1.17 ToString at a String-method slot: the slot takes every
// value, and the lane used to admit String | Undefined by name.
const toB = { toString() { return "b"; } };
const dash = { toString() { return "-"; } };

// §22.1.3.16.1 step 5 -- padStart / padEnd fill string.
console.log("x".padStart(4, 12));
console.log("x".padEnd(4, true));
console.log("x".padStart(4, dash));
console.log("x".padStart(4, null));

// §22.1.3.9 / §22.1.3.11 step 3 -- the search string (2-arg form).
console.log("abcabc".indexOf(toB, 0));
console.log("abcabc".lastIndexOf(toB, 5));
console.log("a1b".indexOf(1, 0));

// §22.1.3.14 step 3 -- the normalization form: undefined takes "NFC",
// everything else is ToString'd and then validated (step 4).
console.log("é".normalize({ toString() { return "NFD"; } }).length);
try {
  console.log("a".normalize(null));
} catch (e) {
  console.log((e as Error).constructor.name);
}

// A throwing toString propagates from each of the three slots.
const boom = { toString(): string { throw new Error("boom"); } };
for (const run of [
  () => "x".padStart(4, boom),
  () => "abc".indexOf(boom, 0),
  () => "a".normalize(boom),
]) {
  try { run(); console.log("no throw"); } catch (e) { console.log("caught " + (e as Error).message); }
}
console.log("done");
