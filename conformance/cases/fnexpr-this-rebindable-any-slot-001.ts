// A `this`-using function expression in an `any` slot the program
// later WRITES. The write target is an lvalue — it never reads the
// cell — so it cannot observe the promoted receiver-first ABI, and
// a binding admitted this way keeps every call on the runtime
// FLAG_CLOSURE_RECV_FIRST gate, which reads the answer off the cell
// actually being called. That is what makes a later value that is
// not promoted safe here.
//
// The `any` spelling is the whole license: a binding carrying the
// first initializer's FUNCTION type keeps that signature at its call
// sites, and rebinding is wrong there for reasons that have nothing
// to do with `this`, so those keep the loud reject.

// Rebound to a plain function — the gate takes its untaken arm and
// the plain body is called with its own argv.
var b: any = function () { return typeof this; };
b = function () { return "plain"; };
console.log(b());

// Never rebound: the same binding still answers the receiverless
// `this` (§10.2.1.2).
var a: any = function () { return typeof this; };
console.log(a());

// And through the construct channel, where the receiver is the new
// object rather than undefined.
var e: any = function () { this.n = 3; };
var made = new e();
console.log(made.n);
