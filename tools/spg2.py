import sys,numpy as np,librosa,librosa.display,matplotlib;matplotlib.use('Agg');import matplotlib.pyplot as plt,warnings;warnings.filterwarnings('ignore')
def load(fn,off,dur):
    if fn.endswith('.f32'): x=np.fromfile(fn,dtype='<f4').astype(np.float32);sr=44100
    else: x,sr=librosa.load(fn,sr=None,mono=True)
    x=x[int(off*sr):int((off+dur)*sr)];return x/(np.abs(x).max()+1e-9),sr
items=[a.split(':',3) for a in sys.argv[2:]];fig,ax=plt.subplots(len(items),1,figsize=(12,3.0*len(items)));ax=np.atleast_1d(ax)
for i,(fn,off,dur,lab) in enumerate(items):
    x,sr=load(fn,float(off),float(dur));S=librosa.amplitude_to_db(np.abs(librosa.stft(x,n_fft=1024,hop_length=128)),ref=np.max)
    librosa.display.specshow(S,sr=sr,hop_length=128,x_axis='time',y_axis='linear',ax=ax[i],vmin=-75,vmax=0,cmap='magma');ax[i].set_title(lab);ax[i].set_ylim(0,6000)
plt.tight_layout();plt.savefig(sys.argv[1],dpi=70)
