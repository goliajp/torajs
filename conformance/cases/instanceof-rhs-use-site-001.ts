// rotation 391 — `instanceof`'s right side became an ordinary
// expression in rotation 390, so a bare constructor name there is now
// a real `Expr::Ident` in the arena. Two whole-program analyses read
// every Ident occurrence of a name and had never seen one in this
// position:
//
//   1. the arguments-object binding chain (`safe_binding_chain`),
//      which kills a chain on any use shape it does not recognise —
//      so `result instanceof C` stopped `C`'s body from materialising
//      its `arguments` and the body died on "unknown identifier
//      arguments";
//   2. the builtin-prototype shadow scan, which reads a mention of a
//      builtin constructor as an escape unless the position only
//      reads through it — so `x instanceof Object` stood the whole
//      Object family's typed tier down, dropping `.then` to the any
//      lane where a bare fn-name argument is not boxable
//      ("box_to_any element type FnSig not supported").
//
// Neither position reads nor writes through the operand: §7.3.22 gets
// `C.prototype` off it and hands `C` itself to nobody.

// (1) the escaping-`arguments` shape: the body stores the whole
// object, so the chain must survive to materialise it.
var args: any;
var C = function () {
  args = arguments;
};
C(7, 8, 9);
console.log(args.length, args[0], args[2]);
const probe: any = { k: 1 };
console.log(probe instanceof C);

// (2) bare `Object` on the right, plus a promise chain settled with
// bare function names — the pair that failed together.
function report(tag: any = undefined): void {
  console.log("settled", tag);
}
async function* gen() {
  yield "v";
}
const plain: any = { a: 1 };
console.log(plain instanceof Object);
console.log("s" instanceof Object);

const p = gen().next();
p.then(report, report).then(report, report);
