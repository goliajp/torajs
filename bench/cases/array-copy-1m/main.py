src = []
i = 0
while i < 10000000:
    src.append(i)
    i += 1
dst = []
j = 0
while j < len(src):
    dst.append(src[j])
    j += 1
print(len(dst) + dst[9999999])
