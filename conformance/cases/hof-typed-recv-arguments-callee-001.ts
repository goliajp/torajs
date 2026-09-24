// A declaration that reads `arguments`, passed to a callback slot on a
// TYPED receiver (and to Array.from), sees the whole argument list.
// Its definition carries hidden argc / argv slots a raw function-pointer
// value does not show; the lane used to hand it the first callback
// argument as the argument count.
function cl(v: number) { return arguments.length; }
function cv(v: number) { return arguments.length + ":" + arguments[1]; }
const xs: number[] = [5, 6];
console.log(xs.map(cl).join(), xs.map(cv).join(), xs.filter(cl).length);
xs.forEach(cv);
console.log(Array.from(xs, cl).join(), Array.from(xs, cv).join());
function none() {
  return arguments.length + ":" + arguments[0];
}
console.log(xs.map(none).join(), xs.some(none), xs.every(none));
