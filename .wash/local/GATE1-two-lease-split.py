import re,sys
log=open('GATE1-two-lease-rv64.console',errors='replace').read().splitlines()
recs=[];samples=[]
for l in log:
    l=l.rstrip('\r')
    if l.startswith('SCHED-TRACE '):
        f=l.split();recs.append((int(f[1]),int(f[2]),f[3],int(f[4]),int(f[5],16)))
    elif l.startswith('LATENCY-SAMPLE gate deadline_notice'):
        f=l.split();samples.append((int(f[3]),int(f[4])))
X=[(i,r) for i,r in enumerate(recs) if r[2]=='X']
Y=[(i,r) for i,r in enumerate(recs) if r[2]=='Y']
U=[r for r in recs if r[2]=='U'];V=[r for r in recs if r[2]=='V']
aud=list(zip([u[4] for u in U],[v[4] for v in V]))
def ins(a,b): return sum(max(0,min(e,b)-max(s,a)) for s,e in aud)
def net(a,b): return (b-a)-ins(a,b)
# group samples by deadline
leases={}
for end,g in samples: leases.setdefault(end-g,[]).append(end)
rows=[]
for d in sorted(leases):
    r1,r2=sorted(leases[d])
    xi,x=next((i,r) for i,r in X if r[4]>=d)
    yi,y=next((i,r) for i,r in Y if i>xi)
    xt,yt=x[4],y[4]
    rows.append((d,net(d,xt),net(xt,yt),net(yt,r1),net(r1,r2),xt-d,yt-xt,r1-yt,r2-r1,xi,yi,x[3]))
print("d | a | b | c1 | c2 | gross a b c1 c2 | X id")
for r in rows: print(r[0],'|',r[1],'|',r[2],'|',r[3],'|',r[4],'|',r[5],r[6],r[7],r[8],'|',r[11])
def pct(v,p):
    v=sorted(v); import math; return v[min(len(v)-1,max(0,math.ceil(p/100*len(v))-1))]
for k,n in zip(range(1,5),'a b c1 c2'.split()):
    v=[r[k] for r in rows]; print(n,'p50/p99',pct(v,50),pct(v,99))
import pickle; pickle.dump((rows,),open('/tmp/rows.pkl','wb'))
print("----")
timed=[(i,r) for i,r in enumerate(recs) if r[2] in 'XYUV']
def idx_at(t):
    # last timed record index with time <= t
    j=0
    for i,r in timed:
        if r[4]<=t: j=i
        else: break
    return j
for row in rows:
    d,r1,r2=row[0],None,None
    ends=sorted(leases[d]); r1,r2=ends
    yi=row[10]
    i1=idx_at(r1); i2=idx_at(r2)
    # trailing window to next timed record after r2
    nxt=next(i for i,r in timed if r[4]>r2)
    seg=recs[yi:nxt+1]
    ks=[]
    for r in seg:
        if r[2] in 'KWRD' : ks.append(f"{r[2]}{r[3]}" + (f"@{r[4]:x}" if r[2]=='K' else ''))
    # compress
    out=[];prev=None;c=0
    for k in ks:
        kk=k.split('@')[0]
        if kk==prev: c+=1
        else:
            if prev: out.append(prev+(f'x{c}' if c>1 else ''))
            prev=kk;c=1
    out.append(prev+(f'x{c}' if c>1 else ''))
    print(row[11], 'Y..after r2 (records',yi,nxt,') r1 near',i1,'r2 near',i2, ':', ' '.join(out[:60]))
print("====")
for row in rows:
    yi=row[10]; seg=recs[yi:yi+40]; out=[]
    for r in seg:
        if r[2] in 'KWR' and r[3] not in (2,):
            out.append(f"{r[2]}{r[3]}:{r[4]&0xffffffffffff:x}")
        if r[2]=='D' and r[3]==42: out.append('D42'); 
        if len(out)>=14: break
    print(row[11], ' '.join(out))
