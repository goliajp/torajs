package main

import "fmt"

func isPrime(n uint64) bool {
	if n < 2 {
		return false
	}
	for i := uint64(2); i*i <= n; i++ {
		if n%i == 0 {
			return false
		}
	}
	return true
}

func main() {
	var count uint64 = 0
	for n := uint64(0); n < 1_000_000; n++ {
		if isPrime(n) {
			count++
		}
	}
	fmt.Println(count)
}
