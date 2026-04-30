function f(): number {
  try {
    return 1;
  } finally {
    console.log("ran finally");
  }
}
console.log(f());
