// A literal array the program deletes from stays typed when its
// element kind keeps a hole in its own slots (numbers, strings,
// object / array cells); booleans and mixed literals still widen.
// Every own-property consumer answers as bun does either way.

var nums = [1, 2.5, 3, -4]
var gone = delete nums[1]
console.log(gone, nums[1], 1 in nums, nums.length, nums.join("|"))
console.log(Object.keys(nums).join(","), JSON.stringify(nums))
var total = 0
for (var i = 0; i < nums.length; i++) {
  if (nums[i] !== undefined) total = total + nums[i]
}
console.log(total, nums.indexOf(3), nums.includes(undefined as any))

var strs = ["a", "b", "c"]
delete strs[0]
console.log(strs[0], 0 in strs, strs.length, strs.join("-"), JSON.stringify(strs))

var objs = [{ v: 1 }, { v: 2 }, { v: 3 }]
delete objs[2]
console.log(objs[2], 2 in objs, objs.length, objs.map((o) => (o ? o.v : 0)).join(","))

var rows = [[1], [2, 2]]
delete rows[0]
console.log(rows[0], rows.length, rows[1].length)

var flags = [true, false, true]
delete flags[1]
console.log(flags[1], 1 in flags, flags.length)

var mixed = [1, "two", 3]
delete mixed[0]
console.log(mixed[0], mixed.length, JSON.stringify(mixed))

// a deleted tail: the length stays, the slot reads undefined
var tail = [9, 8, 7]
delete tail[2]
console.log(tail.length, tail[2], tail.lastIndexOf(7))
