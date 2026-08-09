// Rest-param FnDecls called through the any lane: the boxed adapter
// collects argv[fixed..argc] into a fresh Arr<Any> (§10.2.1.3), and
// forwarders spread-forward their rest param so apply_rest_args does
// not re-pack it (the [null,true,null]-vs-[[],[7],[]] family).
function fa(...args: any[]) { console.log(args); }
var ga: any = fa;
ga();
ga(7);
ga(1, 2, 3);

function outer() {
  var captured = 7;
  function f(...args: any[]) {
    console.log(args, captured);
  }
  var g: any = f;
  g();
  g(8);
}
outer();

function fb(a: any, ...rest: any[]) { console.log(a, rest); }
var gb: any = fb;
gb();
gb(1);
gb(1, 2, 3);

function fc(...xs: any[]) { console.log(xs.length, xs); }
var gc: any = fc;
gc.call(null, 5, 6);
gc.apply(null, [7, 8, 9]);
