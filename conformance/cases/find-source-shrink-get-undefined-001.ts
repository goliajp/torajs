// find / findIndex have no HasProperty step (§23.1.3.9): an index the
// predicate cut off is still visited, and Get answers undefined for it.
var arr = ['Shoes', 'Car', 'Bike'];
var results: any[] = [];
arr.find(function (kValue) {
  if (results.length === 0) arr.splice(1, 1);
  results.push(kValue);
});
console.log(results.length, results[0], results[1], results[2]);
var nums = ["p", "q", "r"];
var seen: any[] = [];
console.log(nums.findIndex(function (v) { nums.length = 1; seen.push(v); return false; }), seen.join());
var arr2 = ['Skateboard', 'Barefoot'];
var r2: any[] = [];
arr2.find(function (k) { if (r2.length === 0) { arr2.push('Motorcycle'); arr2[1] = 'Magic Carpet'; } r2.push(k); });
console.log(r2.join());
