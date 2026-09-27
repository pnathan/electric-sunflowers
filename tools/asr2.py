import sys,whisper,warnings,re,jiwer,json,subprocess;warnings.filterwarnings('ignore')
m=whisper.load_model('base.en');L=json.loads(subprocess.check_output(['node','-e',"console.log(JSON.stringify(require('./tests/words.js').LINES))"]))
clean=lambda s:re.sub(r'[^a-z\' ]','',s.lower().replace('-',' ')).strip()
for tag in sys.argv[1:]:
    refs,hyps,vks=[],[],[]
    import os
    for vk in os.environ.get('VOICES','baritone,alto').split(','):
        for i,l in enumerate(L):
            r=clean(m.transcribe(f'ref/s_{tag}_{vk}_{i}.wav',language='en',fp16=False,temperature=0,condition_on_previous_text=False)['text'])
            rr=clean(' '.join(l));r=' '.join(r.split()[:len(rr.split())+3]);refs.append(rr);hyps.append(r);vks.append(vk)
    # consonant-level: count reference words beginning with d/t that appear in the hypothesis
    dt=[w for r in refs for w in r.split() if w[0] in 'dt'];hit=0
    for r,h in zip(refs,hyps):
        hw=set(h.split());hit+=sum(1 for w in r.split() if w[0] in 'dt' and w in hw)
    per={v:jiwer.wer([r for r,k in zip(refs,vks) if k==v],[h for h,k in zip(hyps,vks) if k==v]) for v in set(vks)}
    print('%-10s WER all %.3f  baritone %.3f  alto %.3f   d/t-initial words recognised %d/%d'%(tag,jiwer.wer(refs,hyps),per.get('baritone',-1),per.get('alto',-1),hit,len(dt)))
    if len(sys.argv)==2:
        for r,h in zip(refs,hyps):print('   %-42s| %s'%(r,h))
