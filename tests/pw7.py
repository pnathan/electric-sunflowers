import asyncio,os
ROOT=os.path.abspath(os.path.join(os.path.dirname(__file__),'..'))
PAGE='file://'+ROOT+'/dist/singer-songwriter-bot.html'
LAUNCH=dict(executable_path=os.environ['CHROME']) if os.environ.get('CHROME') else {}
from playwright.async_api import async_playwright
INIT="""window.__saved=[];window.claude={use:async n=>{if(n==='downloads')return {save:async({filename,data})=>{window.__saved.push({filename,data});return {status:'saved'};}};return null;}};"""
async def main():
    async with async_playwright() as p:
        b=await p.chromium.launch(**LAUNCH,args=['--autoplay-policy=no-user-gesture-required'])
        pg=await b.new_page(viewport={'width':1280,'height':900});errs=[];pg.on('pageerror',lambda e: errs.append(str(e)));pg.on('console',lambda m: errs.append(m.text) if m.type=='error' else None)
        await pg.add_init_script(INIT);await pg.goto(PAGE);await pg.wait_for_timeout(500)
        t=await pg.evaluate('Date.now()');await pg.click('#demo')
        await pg.wait_for_function('!document.querySelector("#play").disabled',timeout=240000);print('ready ms',await pg.evaluate('Date.now()')-t)
        await pg.click('.vt[data-v=score]');await pg.wait_for_timeout(20000)
        await pg.screenshot(path='sc_light.png')
        t=await pg.evaluate('Date.now()');await pg.click('#b-harp');await pg.wait_for_function('!document.querySelector("#saveAudio").disabled',timeout=60000);print('remix ms',await pg.evaluate('Date.now()')-t)
        await pg.click('#play')
        t=await pg.evaluate('Date.now()');await pg.click('#saveAudio');await pg.wait_for_function('window.__saved.length>0',timeout=120000);print('encode ms',await pg.evaluate('Date.now()')-t)
        await pg.click('#saveScore');await pg.wait_for_function('window.__saved.length>1',timeout=20000)
        r=await pg.evaluate('''(async()=>{const a=window.__saved[0],s=window.__saved[1];const ab=await a.data.arrayBuffer();const ac=new OfflineAudioContext(2,44100,44100);
          let dec;try{dec=await ac.decodeAudioData(ab.slice(0));}catch(e){return {name:a.filename,err:String(e),bytes:ab.byteLength};}
          // correlate decoded (48k) against mix (44.1k) over a 5 s window at 60 s, allowing a small lag
          const m=ST.mixData.L,d=dec.getChannelData(0),sr=dec.sampleRate;let best=-1,bl=0;
          for(let lag=-600;lag<=600;lag+=4){let c=0,e1=0,e2=0;for(let i=0;i<44100*2;i+=3){const t=60+i/44100;const j=Math.round(t*sr)+lag;const x=m[Math.round(t*44100)],y=d[j];c+=x*y;e1+=x*x;e2+=y*y;}const r=c/Math.sqrt(e1*e2);if(r>best){best=r;bl=lag;}}
          return {name:a.filename,bytes:ab.byteLength,decDur:dec.duration,mixDur:ST.buf.duration,sr,corr:best,lag:bl,svg:s.filename,svgBytes:s.data.size};})()''')
        print(r)
        await pg.emulate_media(color_scheme='dark');await pg.wait_for_timeout(300);await pg.screenshot(path='sc_dark.png')
        print(errs[:8]);await b.close()
asyncio.run(main())
