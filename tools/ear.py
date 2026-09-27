import sys,numpy as np,warnings,librosa;warnings.filterwarnings('ignore')
from panns_inference import AudioTagging,labels
at=AudioTagging(checkpoint_path='/root/panns_data/Cnn14_mAP=0.431.pth',device='cpu')
def load(fn):
    if fn.endswith('.f32'): x=np.fromfile(fn,dtype='<f4').astype(np.float32);sr=44100
    else: x,sr=librosa.load(fn,sr=None,mono=True)
    x=librosa.resample(x,orig_sr=sr,target_sr=32000);return x/(np.abs(x).max()+1e-9)
watch=sys.argv[1].split('|');
for fn in sys.argv[2:]:
    x=load(fn)[:32000*20];cw,_=at.inference(x[None,:]);p=cw[0];top=np.argsort(p)[::-1][:5]
    w=' '.join('%s=%.2f'%(k,p[labels.index(k)]) for k in watch)
    print(fn.split('/')[-1].ljust(16),w,'| top:',', '.join('%s %.2f'%(labels[i],p[i]) for i in top))
