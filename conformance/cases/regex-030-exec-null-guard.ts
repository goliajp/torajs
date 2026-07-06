// RC-4 F1a part 2 (RFC 20260706-test262-bug-corpus) — un-narrowed
// deref of a missed exec/match result is a catchable TypeError
// (spec §13.3.2.1 null-deref), not a SIGSEGV. The lowering emits
// __torajs_arr_null_check + throw-check in front of the inline
// .length / element loads when the receiver is a nullable-arr
// source (exec/match let-init or direct call chain).

let miss = /zzz/.exec("abc");
try {
  console.log(miss.length);
} catch (e) {
  console.log("caught-len", e instanceof TypeError);
}
try {
  console.log(miss[0]);
} catch (e) {
  console.log("caught-idx", e instanceof TypeError);
}

// match mirrors exec.
let mm = "hello".match(/zzz/);
try {
  console.log(mm[0]);
} catch (e) {
  console.log("caught-match", e instanceof TypeError);
}

// Direct call-chain receiver.
try {
  console.log(/nope/.exec("abc").length);
} catch (e) {
  console.log("caught-chain", e instanceof TypeError);
}

// Hit path keeps working through the guard.
let hit = /(b)/.exec("abc");
console.log(hit.length, hit[0], hit[1]);
console.log("done");
