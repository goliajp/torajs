// RFC 20260721 刀 12 G16 — for-in over a TYPED array re-checks each
// snapshot key against the live receiver (ES §14.7.5: a property
// removed mid-enumeration is never visited): a mid-loop shrink
// (`length =`) stops the walk, and splice moves the exotic HOLE
// shadows with their elements so an unvisited deleted index answers
// absent.
let a = [0, 1];
let iterations = 0;
for (let k in a) {
  iterations++;
  a.length = 1;
}
console.log("shrink:", iterations);
let arr = [0, 1, 2, 3, 4, 5, , 7];
let seen: string[] = [];
for (let p in arr) {
  if (p === "1") {
    arr.splice(2, 3);
  }
  seen.push(p);
}
console.log("splice:", seen.join("|"));
console.log("after:", arr.length, arr.join(","));
let b: any = [10, 20, 30];
let seen2: string[] = [];
for (let p in b) {
  if (p === "0") {
    b.length = 2;
  }
  seen2.push(p);
}
console.log("any shrink:", seen2.join("|"));
