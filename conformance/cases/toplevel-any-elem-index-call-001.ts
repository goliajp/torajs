// An array of closures at the top level, called through an index on
// the MAIN path, in a program where some named fn also reads it.
//
// A named fn body reads top-level bindings through the globals
// table, so the binding gets promoted — and the slot K.3b infers for
// an array of closures is `any[]`: the elements are boxes. The
// checker's main-side view of the same binding still carries the
// array literal's own element type, so `arr[0]` types as a function
// while the value the call has to work with is an Any. The typed
// indirect lane does not claim that operand, and the call was a hard
// reject ("unsupported callee form: Index") — on an ordinary program,
// and only from the moment some named fn mentioned the binding.
// Spelling the same array `const arr: any[] = [...]` worked, because
// then both homes agree.
//
// Reading the slot rather than the checker answers it, which is the
// rule the bare-Ident callee already follows.
const strs = [(a: number) => "x" + a];
function viaNamedStr() {
  return strs[0](3);
}
console.log(strs[0](1), viaNamedStr());

// A number-returning element: the any lane's result has to come back
// as the number the checker promised, not a box.
const nums = [(a: number) => a + 1];
function viaNamedNum() {
  return nums[0](3);
}
console.log(nums[0](1) + 10, nums[0](1) === 2, typeof nums[0](1), viaNamedNum());

// And a string-returning one used as a string.
console.log(strs[0](1) + "!", strs[0](1).length);

// A fn-body binding that SHADOWS the top-level name reads its own
// array, not the global of the same name.
function shadows() {
  const nums = [(a: number) => a * 2];
  return nums[0](3);
}
console.log(shadows(), nums[0](1));

// The receiver is still the array (§13.3.6.2), on both paths.
const rows = [[(a: number) => a]];
function viaNamedRow() {
  return rows[0].length;
}
console.log(rows[0][0](5), viaNamedRow());
