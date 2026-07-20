// RFC 20260721 刀 5 follow-up — the copy family (slice / toSorted /
// toReversed) rides [[Get]] on an exotic-index receiver: an accessor
// index reads through its getter (which may mutate the receiver; the
// length snapshot holds), never the dead raw slot.
var arr: any = [5, 0, 3];
var calls = 0;
Object.defineProperty(arr, "0", {
  get: function () {
    calls = calls + 1;
    if (calls === 1) {
      arr.push(1);
    }
    return 5;
  },
});
console.log("toSorted:", JSON.stringify(arr.toSorted()));
console.log("arr after:", JSON.stringify(arr));
console.log("slice:", JSON.stringify(arr.slice(0, 3)));
console.log("toReversed:", JSON.stringify(arr.toReversed()));
console.log("getter ran:", calls > 0);
