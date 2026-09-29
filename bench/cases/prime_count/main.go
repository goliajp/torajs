package main

import (
	"fmt"
	"os"
	"strconv"
)

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
	limit, _ := strconv.ParseUint(os.Args[1], 10, 64)
	var count uint64 = 0
	for n := uint64(0); n < limit; n++ {
		if isPrime(n) {
			count++
		}
	}
	fmt.Println(count)
}
