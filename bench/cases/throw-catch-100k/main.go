package main

import "fmt"

func trial(i int64) (result int64) {
	defer func() {
		if r := recover(); r != nil {
			result = r.(int64)
		}
	}()
	panic(i)
}

func main() {
	var total int64 = 0
	for i := int64(0); i < 100000; i++ {
		total = total + trial(i)
	}
	fmt.Println(total)
}
