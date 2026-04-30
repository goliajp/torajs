def loop_sum(n):
    s = 0
    for i in range(n):
        p = {"fst": i, "snd": i + 1}
        s = s + p["fst"] + p["snd"]
    return s


print(loop_sum(1_000_000))
