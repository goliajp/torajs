// chunk 740 — mutable closure-captured toplevel bindings promote to
// globals (RFC 20260709 residual): the capture filter resolves reads
// to the global and the lifted body's writes take the Assign-Ident
// global lane, so the slot is the single home (the old env-copy
// snapshot disagreed with ES shared-binding semantics)

// Copy-type counter: closure writes, named fn reads
let counter = 0;
const inc = () => {
  counter = counter + 1;
};
function show(): void {
  console.log(counter);
}
inc();
inc();
show();
console.log(counter);

// Str: closure append, named fn reads
let msg = "a";
const app = () => {
  msg = msg + "x";
};
function showMsg(): void {
  console.log(msg);
}
app();
showMsg();

// Closure slot: closure swaps it, named fn calls it
let cb = (n: number) => n + 1;
const swap = () => {
  cb = (n: number) => n * 10;
};
function run(): void {
  console.log(cb(5));
}
run();
swap();
run();

// multiple closures share one binding
let shared = 10;
const w1 = () => {
  shared = shared + 1;
};
const w2 = () => {
  shared = shared * 2;
};
const rd = () => shared;
w1();
w2();
console.log(rd());
console.log(shared);

// fn-local shadow stays local; outer mutation still visible
let s = "top";
const f = () => {
  let s = "local";
  return s + "!";
};
const g = () => {
  s = s + "-mut";
};
function showS(): void {
  console.log(s);
}
console.log(f());
g();
showS();

// mutable + captured + named-fn init (forwarder wrap axis)
function take(x: number): number {
  return x + 1;
}
let fcb = take;
const fswap = () => {
  fcb = (x: number) => x * 3;
};
function frun(): void {
  console.log(fcb(4));
}
frun();
fswap();
frun();
