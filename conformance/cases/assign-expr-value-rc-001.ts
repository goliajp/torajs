// An assignment expression's value, when a consumer keeps it, is a
// reference of its own — not a borrow of the slot that was just written.
//
// `b = (a = [1,2,3])` leaves the array in two places. Both release at
// scope end, so the assignment must hand its consumer a stake rather
// than lending out the one the slot holds. Getting that wrong is
// invisible until teardown: the second release underflows the refcount
// on already-freed memory, and the corpse sits in the cycle-root buffer
// until `__torajs_cycle_at_exit_drain` walks it and segfaults.
//
// The reads below are all AFTER the writes, so a stolen stake shows up
// as wrong output rather than only as a crash.

// kept by a second binding
var a: any, b: any;
b = (a = [1, 2, 3]);
console.log(String(a), String(b));

// kept by a declaration initializer
var c: any;
var d: any = (c = [4, 5]);
console.log(String(c), String(d));

// chained — every link keeps it
var e: any, f: any, g: any;
g = (f = (e = [6, 7]));
console.log(String(e), String(f), String(g));

// kept by an object literal, an array literal, and a call argument
var h: any;
var obj = { k: (h = [8, 9]) };
console.log(String(h), String(obj.k));

var i: any;
var arr = [(i = [10])];
console.log(String(i), String(arr[0]));

var j: any;
function idlen(x: any): number {
  return x.length;
}
console.log(idlen((j = [11, 12, 13])), String(j));

// kept across a return
var k: any;
function assignAndReturn(): any {
  return (k = [14, 15]);
}
console.log(String(assignAndReturn()), String(k));

// discarded — the statement form must release, not leak
var m: any;
m = [16];
m = [17];
console.log(String(m));

// self-assignment must not release what it is about to store
var n: any = [18, 19];
n = n;
console.log(String(n));

// closures capturing hoisted vars, written from a for head and an
// unbraced multi-declarator body — the shape test262's
// language/statements/for/scope-body-var-none.js reaches
var probeTest: any, probeIncr: any, probeBody: any;
var run = true;
for (
  ;
  run && (probeTest = function () { return [x, z]; });
  probeIncr = function () { return [x, z]; }
)
  var z = 1, _ = (probeBody = function () { return [x, z]; }), run = false;
var x = 2;
console.log(String(probeTest()), String(probeBody()), String(probeIncr()));
