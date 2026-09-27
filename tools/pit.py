import numpy as np,librosa,warnings,sys;warnings.filterwarnings('ignore')
x=np.fromfile(sys.argv[1],dtype='<f4').astype(np.float32);sr=44100
t=float(sys.argv[2]);ms=[int(v) for v in sys.argv[3].split(',')];dt=float(sys.argv[4])
for i,m in enumerate(ms):
    a=int((t+i*dt+0.3)*sr);seg=x[a:a+int(0.5*sr)]
    f0,vf,vp=librosa.pyin(seg,fmin=100,fmax=1500,sr=sr,frame_length=4096)
    f=np.nanmedian(f0);exp=440*2**((m-69)/12)
    # subharmonic energy: spectrum level at f/2 relative to f
    S=np.abs(np.fft.rfft(seg*np.hanning(len(seg))));fr=np.fft.rfftfreq(len(seg),1/sr)
    lv=lambda q:20*np.log10(S[(fr>q*0.97)&(fr<q*1.03)].max()+1e-9)
    print('m%d exp %.1f got %.1f cents %+.0f  sub(f/2)-f %.1f dB  voiced %.2f'%(m,exp,f,1200*np.log2(f/exp) if f==f else 0,lv(exp/2)-lv(exp),np.mean(vf)))
