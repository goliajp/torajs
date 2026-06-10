// throw of a borrowed (non-Copy param) binding must hand the catch a
// +1 reference of its own: the caller's binding stays the canonical
// owner and is read again AFTER the catch scope closed and dropped
// the caught value. Without retain-at-throw the catch drop releases
// the owner's stake and the trailing read is use-after-free.
function boom(s: string): void {
  throw s;
}

function main(): void {
  const msg: string = "borrowed-throw-payload";
  try {
    boom(msg);
  } catch (e) {
    console.log(e);
  }
  console.log(msg);
}

main();
