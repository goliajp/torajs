package main

import "fmt"

type Pair[A any, B any] struct {
	Fst A
	Snd B
}

func loopSum(n int64) int64 {
	var sum int64 = 0
	for i := int64(0); i < n; i++ {
		p := &Pair[int64, int64]{Fst: i, Snd: i + 1}
		sum = sum + p.Fst + p.Snd
	}
	return sum
}

func main() {
	fmt.Println(loopSum(1000000))
}
