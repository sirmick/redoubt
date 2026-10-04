import random
LIMIT=2048
def pieces(e):
    e=list(e); n=0
    while True:
        end=len(e); split=0
        while end-split>1:
            if end-split<0xff and sum(e[split:])<=LIMIT: break
            split+=(end-split)//2
        if split==0: return n
        e=e[:split]; n+=1
def search(TMAX, EMIN, EMAX, iters=200000, seed=1):
    random.seed(seed); best=0; bestE=None
    cur=[EMIN]*10
    for it in range(iters):
        e=list(cur)
        op=random.random()
        if op<0.3 and len(e)>1: e.pop(random.randrange(len(e)))
        elif op<0.6: e.insert(random.randrange(len(e)+1), random.randint(EMIN,EMAX))
        else:
            i=random.randrange(len(e)); e[i]=random.randint(EMIN,EMAX)
        if sum(e)>TMAX or len(e)>300: continue
        p=pieces(e)
        if p>=pieces(cur) or random.random()<0.01: cur=e
        if p>best: best=p; bestE=e
    return best,bestE
for TMAX,EMAX in [(4096+1030,1100),(4096+1030,4000),(8192,4000),(4096+300,300),(4096+300,60)]:
    b=max(search(TMAX,25,EMAX,100000,s)[0] for s in range(3))
    print(TMAX,EMAX,b)
