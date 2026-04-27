package main

import "fmt"

func popcount(x uint64) uint64 {
	n := x
	count := uint64(0)
	for n != 0 {
		n = n & (n - 1)
		count++
	}
	return count
}

func main() {
	var total uint64 = 0
	for i := uint64(0); i < 10000000; i++ {
		total += popcount(i)
	}
	fmt.Println(total)
}
