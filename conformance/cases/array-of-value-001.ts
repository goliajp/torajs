// Array.of read as a VALUE — reflection + detached call (§23.1.2.3)
console.log(typeof Array.of, Array.of.length, Array.of.name);
const detached = Array.of;
const a = detached(1, "two", true);
console.log(a.length, a[0], a[1], a[2]);
const empty = detached();
console.log(empty.length);
// direct call keeps working
console.log(Array.of(7, 8).length);
