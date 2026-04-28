package main

import "fmt"

func steps(n uint64) uint64 {
	var count uint64 = 0
	for n != 1 {
		if n&1 == 0 {
			n >>= 1
		} else {
			n = 3*n + 1
		}
		count++
	}
	return count
}

func main() {
	var max uint64 = 0
	for i := uint64(1); i <= 1_000_000; i++ {
		s := steps(i)
		if s > max {
			max = s
		}
	}
	fmt.Println(max)
}
