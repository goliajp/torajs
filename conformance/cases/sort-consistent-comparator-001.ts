// §23.1.3.30 leaves the order implementation-defined when the
// comparator is not consistent, so bun and tr may legally disagree
// there — `(a, b) => a < b ? -1 : 0` says cmp(3,1) is 0 while
// cmp(1,3) is -1, and the two answers are both allowed.
//
// What is NOT implementation-defined is a CONSISTENT comparator, and
// this pins those: the kernel's ordering predicate is "the comparator
// said strictly greater", which is only sound while the comparator is
// antisymmetric. Anything that changes how the comparator's result
// reaches that predicate has to keep every line below.
const nums = [5, 2, 9, 1, 5, 0];
console.log(nums.slice().sort((a, b) => a - b).join(","));
console.log(nums.slice().sort((a, b) => b - a).join(","));
console.log(
  nums
    .slice()
    .sort((a, b) => {
      if (a < b) {
        return -1;
      }
      if (a > b) {
        return 1;
      }
      return 0;
    })
    .join(","),
);

const words = ["pear", "fig", "apple", "kiwi"];
console.log(words.slice().sort((a, b) => a.length - b.length).join(","));
console.log(words.slice().toSorted((a, b) => b.length - a.length).join(","));

// No comparator at all: §23.1.3.30 step 3 sorts by the string form.
console.log([10, 9, 80, 1].slice().sort().join(","));
