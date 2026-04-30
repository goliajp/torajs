// Adapted from test262: function declared `: void` produces no value.
// We verify by side-effect (console.log inside fn produces output).
function shout(msg: string): void {
  console.log(msg);
}

function check(): number {
  shout("hi");
  shout("bye");
  return 0;
}
console.log(check());
