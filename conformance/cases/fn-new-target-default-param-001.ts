// A function expression reading new.target sees the constructor under
// `new` even when it declares a parameter default, or lives in a
// function whose parameters carry no annotation; its arguments count
// leaves the receiver and new.target out.
function outer(x) {
  const F: any = function (y: any = 5) {
    console.log(new.target === F, y, arguments.length, x);
  };
  new F();
  new F(2);
  F(3);
}
outer(1);
outer("s");
