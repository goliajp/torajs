// arguments.length write face — length-write knife (rotation 270).
// FoldTo bodies that WRITE arguments.length move to LiveLength: reads
// and writes both ride the materialized array's live .length; Real
// (named-fn) bodies ride __torajs_real_argc, post-incr included.

// 1. iterator expansion after length grow (ArrayIteratorPrototype
//    args-*-expansion-* shape): insertion honored before exhaustion.
(function (a, b, c) {
  var iterator = arguments[Symbol.iterator]();
  iterator.next();
  iterator.next();
  arguments.length = 4;
  arguments[3] = 5;
  var r = iterator.next();
  console.log(r.value, r.done);
  r = iterator.next();
  console.log(r.value, r.done);
  r = iterator.next();
  console.log(r.value, r.done);
})(2, 1, 3);

// 2. read-after-write: grow then truncate, mapped index read intact.
(function (x) {
  arguments.length = 3;
  console.log(arguments.length);
  arguments.length = 1;
  console.log(arguments.length, arguments[0]);
})(9);

// 3. named fn (Real tier): write + post-decr countdown loop.
function realTierWrite(p, q) {
  arguments.length = 4;
  console.log(arguments.length);
  var n = 0;
  while (arguments.length > 0) {
    arguments.length--;
    n++;
    if (n > 10) break;
  }
  console.log(arguments.length, n);
}
realTierWrite(1, 2);

// 4. truncation before exhaustion (args-*-truncation-* shape).
(function (a, b, c) {
  var it = arguments[Symbol.iterator]();
  it.next();
  arguments.length = 1;
  var r = it.next();
  console.log(r.value, r.done);
})(7, 8, 9);

// 5. length-only IIFE (Real tier via iife_real_argc): plain write.
(function () {
  arguments.length = 2;
  console.log(arguments.length);
})();

// 6. spread reads the LIVE array after a grow, not a stale prefix.
(function (a, b) {
  arguments.length = 3;
  arguments[2] = 7;
  console.log([...arguments].join(","));
})(1, 2);

// 7. compound assignment desugars through the same write face.
(function (a, b, c) {
  arguments.length -= 2;
  console.log(arguments.length, arguments[0]);
})(4, 5, 6);

// 8. post-decr on a LiveLength body (non-length touch + length--).
(function (x, y) {
  var t = arguments[x];
  arguments.length--;
  console.log(arguments.length, t);
})(1, 9);
