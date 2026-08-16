// §23.1.2.3 Array.of reads `this` as C: when C is a constructor the
// allocation goes through `Construct(C, «len»)` rather than
// ArrayCreate, and the items land with CreateDataPropertyOrThrow. The
// t262 spelling of that branch is `Array.of.call(A, …)`, where `A` is
// normally a `this`-writing function expression.
//
// Two halves had to move together: the static's value cell now carries
// the receiver-first flag (so `.call` / `.apply` deliver the thisArg in
// argv[0]), and the promotion lane that decides whether a `this`-using
// function expression keeps its receiver now counts an argument to
// `Array.of.call` as receiver-safe, next to `Array.from` — §23.1.2.3
// does nothing with C except construct through it.

var log: any = [];

var Ctor: any = function (len: any) {
  log.push("ctor:" + len);
  this.tag = "made";
};

var built: any = Array.of.call(Ctor, "a", "b", "c");
console.log(log.join(","));
console.log(built.tag, built.length, built[0], built[2]);

// `.apply` is the same channel
var applied: any = Array.of.apply(Ctor, ["x", "y"]);
console.log(applied.tag, applied.length, applied[1]);

// a non-constructor receiver takes step 4.b's ArrayCreate — the plain
// answer, which is also what a bare call through the value cell gets
var plain: any = Array.of;
var made: any = plain(1, 2, 3);
console.log(made.length, made[0], made[2]);
console.log(Array.of.call(undefined, 7).length);
console.log(Array.of(9).length, Array.of().length);

// the static's own reflection surface is unchanged by the receiver
// flag (§23.1.2.3 is rest-param shaped, so length is 0)
console.log(typeof plain, plain.length);
