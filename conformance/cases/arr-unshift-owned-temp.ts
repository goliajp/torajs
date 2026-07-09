// chunk 742 - unshift owned-temp release (push twin, chunk 733): an
// owned-shape arg (closure literal / array literal / call result)
// hands its +1 to the array; borrow shapes (ident reads) keep their
// binding's stake untouched
const keep = [9, 8];
const xs: number[][] = [[1]];
xs.unshift(keep);
xs.unshift([5, 6]);
console.log(xs.length, xs[0][0], xs[1][0], xs[2][0]);
console.log(keep[0], keep[1]);

const fns: Array<() => string> = [];
const named = () => "n";
fns.unshift(named);
fns.unshift(() => "lit");
console.log(fns[0](), fns[1]());
console.log(named());

const ss: string[] = ["z"];
const s0 = "borrowed";
ss.unshift(s0);
ss.unshift("owned" + "!");
console.log(ss.join(","));
console.log(s0);
