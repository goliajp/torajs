package main

import "fmt"

func id[T any](x T) T {
	return x
}

func loopSum(xs []int64) int64 {
	var sum int64 = 0
	for _, x := range xs {
		sum = sum + id(x)
	}
	return sum
}

func main() {
	var xs []int64
	for i := int64(0); i < 10000000; i++ {
		xs = append(xs, i)
	}
	fmt.Println(loopSum(xs))
}
