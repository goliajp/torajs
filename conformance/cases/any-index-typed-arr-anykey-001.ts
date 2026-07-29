// An `any` KEY on a typed array receiver — §7.1.19 ToPropertyKey
// decides at runtime whether a[k] is an element read, a property
// read, or a miss, so the keyed kernel answers all three. The kernel
// reads raw typed slots through the header's element-kind field: a
// heap array records it at the boxing boundary, a non-escaping array
// literal (stack alloca, FLAG_STATIC_LITERAL) is born with it baked
// in — that block refuses the runtime write, so an unbaked kind read
// back as UNSET and every element answered undefined.
var i: any = 1;
var zero: any = 0;
var len: any = "length";
var frac: any = 1.7;
var numeric: any = "1";
var neg: any = -1;
var oob: any = 9;
var bkey: any = true;

// number elements — stack literal
const nums = [10, 20, 30];
console.log(nums[i]);
console.log(nums[len]);
console.log(nums[frac]);
console.log(nums[numeric]);
console.log(nums[neg]);
console.log(nums[oob]);
console.log(nums[bkey]);

// float elements
const floats = [1.5, 2.5];
console.log(floats[i]);

// string elements — refcounted, so a heap array marked at box_to_any
const strs = ["a", "b", "c"];
console.log(strs[i]);
console.log(strs[len]);

// bool elements
const bools = [true, false, true];
console.log(bools[i]);
console.log(bools[zero]);

// nested arrays
const grid = [[1, 2], [3, 4]];
console.log(grid[i][zero]);

// heap array grown at runtime
const grown: number[] = [];
grown.push(7);
grown.push(8);
console.log(grown[i]);
console.log(grown[len]);
