package main

import "fmt"

func add1(x int64) int64 {
	return x + 1
}

func reduce(xs []int64, f func(int64) int64) int64 {
	var sum int64 = 0
	for i := 0; i < len(xs); i++ {
		sum = sum + f(xs[i])
	}
	return sum
}

func main() {
	xs := make([]int64, 0)
	for i := int64(0); i < 10000000; i++ {
		xs = append(xs, i)
	}
	fmt.Println(reduce(xs, add1))
}
