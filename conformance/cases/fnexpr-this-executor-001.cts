// new Promise executor: §27.2.3.1 step 9 Call(executor, undefined, ...)
// — the sloppy goal binds globalThis (ThisMode ~global~).
var _this: any;
new Promise(function (res: any) {
  _this = this;
  res(1);
});
console.log(_this === globalThis);
