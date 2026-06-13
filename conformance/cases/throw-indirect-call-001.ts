function boom(): void { throw "kapow"; }
function callThunk(thunk: () => void): boolean {
  let threw: boolean = false;
  try { thunk(); } catch (e: string) { threw = true; }
  return threw;
}
console.log(callThunk(boom));
console.log(callThunk(function() { throw "anon-boom"; }));
console.log(callThunk(function() {}));
function callAndCatchMsg(thunk: () => void): void {
  try { thunk(); } catch (e: string) { console.log(e); }
}
callAndCatchMsg(function() { throw "msg-via-catch"; });
var deep = function() { boom(); };
console.log(callThunk(deep));
