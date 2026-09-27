import aifc,numpy as np,sys,warnings;warnings.filterwarnings('ignore')
def load(fn):
    a=aifc.open(fn);n=a.getnframes();ch=a.getnchannels();sw=a.getsampwidth();sr=a.getframerate()
    raw=a.readframes(n)
    if sw==3:
        b=np.frombuffer(raw,dtype=np.uint8).reshape(-1,3).astype(np.int32);x=((b[:,0]<<24)|(b[:,1]<<16)|(b[:,2]<<8)).astype(np.int32).astype(float)
    else: x=np.frombuffer(raw,dtype='>i2' if sw==2 else '>i4').astype(float)
    x=x.reshape(-1,ch).mean(1);return x/np.abs(x).max(),sr
BANDS=np.array([63,80,100,125,160,200,250,315,400,500,630,800,1000,1250,1600,2000,2500,3150,4000,5000,6300,8000,10000,12500])
def ltas(x,sr):
    N=8192;w=np.hanning(N);P=np.zeros(N//2+1);c=0
    for i in range(0,len(x)-N,N//2):
        seg=x[i:i+N]
        if np.sqrt((seg**2).mean())<0.02: continue
        P+=np.abs(np.fft.rfft(seg*w))**2;c+=1
    f=np.fft.rfftfreq(N,1/sr);P/=max(c,1)
    out=[]
    for b in BANDS:
        m=(f>=b/2**(1/6))&(f<b*2**(1/6));out.append(10*np.log10(P[m].sum()+1e-20))
    out=np.array(out);return out-out.max()
if __name__=='__main__':
    for fn in sys.argv[1:]:
        x,sr=load(fn);L=ltas(x,sr);print(fn.ljust(8),' '.join('%4d'%v for v in L))
    print('band    ',' '.join('%4d'%(b if b<1000 else b//100) for b in BANDS))
