// §14.12.4 selects a clause with IsStrictlyEqual. A pointer-shaped
// slot spells its undefined with the per-type immortal cell, and
// the `undefined` clause value lowers to the null pointer, so the
// raw compare was never equal: `case undefined:` was never selected
// under exactly the type that exists to carry undefined, and the
// checker warned that it never could be.

const o: { a: number } | undefined = undefined;
switch (o) {
  case undefined:
    console.log("undefined");
    break;
  default:
    console.log("default");
}

// the live side of the same slot matches its own value
const o2: { a: number } | undefined = { a: 1 };
switch (o2) {
  case o2:
    console.log("same");
    break;
  default:
    console.log("other");
}

// `case null:` was already right — NULL is how null is spelled here
const n: { a: number } | null = null;
switch (n) {
  case null:
    console.log("null");
    break;
  default:
    console.log("default");
}
switch (o) {
  case null:
    console.log("null");
    break;
  default:
    console.log("default-for-undefined");
}

// the array and class shapes take the same slot
const xs: number[] | undefined = undefined;
switch (xs) {
  case undefined:
    console.log("xs undefined");
    break;
  default:
    console.log("xs default");
}

// §14.12.4 evaluates the clause expression before comparing, so a
// side effect in a never-matching clause is still observable
let ticks = 0;
function bump(): { a: number } {
  ticks = ticks + 1;
  return { a: 9 };
}
switch (o2) {
  case bump():
    console.log("bumped");
    break;
  default:
    console.log("not bumped");
}
console.log(ticks);

// (a STRING slot spells its undefined with the Str-shaped oddball
// rather than the generic cell, so `case undefined:` under
// `string | undefined` is still never selected. Registered, a
// different in-band spelling.)

// the spellings that were already right keep their arms
const k = 2;
switch (k) {
  case 2:
    console.log("two");
    break;
  default:
    console.log("other");
}

// the same clause under a value the checker types as a bare `T` —
// a `find` miss hands back the cell, and the clause catches it
const os: { a: number }[] = [];
const miss = os.find((x) => true);
switch (miss) {
  case undefined:
    console.log("miss undefined");
    break;
  default:
    console.log("miss default");
}
