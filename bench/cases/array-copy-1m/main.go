package main

import "fmt"

func main() {
	src := make([]int64, 0)
	for i := int64(0); i < 10000000; i++ {
		src = append(src, i)
	}
	dst := make([]int64, 0)
	for i := 0; i < len(src); i++ {
		dst = append(dst, src[i])
	}
	fmt.Println(int64(len(dst)) + dst[9999999])
}
