import asyncio,os
ROOT=os.path.abspath(os.path.join(os.path.dirname(__file__),'..'))
PAGE='file://'+ROOT+'/dist/singer-songwriter-bot.html'
LAUNCH=dict(executable_path=os.environ['CHROME']) if os.environ.get('CHROME') else {}
from playwright.async_api import async_playwright
INIT="""window.claude={use:async n=>{if(n!=='sample')return null;const f=async(p,o)=>{window.__prompt=p;const j=JSON.stringify(DEMO_SONG).replace('Every Harbor','Mock Harbor');o.onText({text:j.slice(0,300)});return {text:j,truncated:false,modelTierApplied:'complex'};};return f;}};"""
async def main():
    async with async_playwright() as p:
        b=await p.chromium.launch(**LAUNCH,args=['--autoplay-policy=no-user-gesture-required'])
        pg=await b.new_page(viewport={'width':1280,'height':1000});errs=[];pg.on('pageerror',lambda e: errs.append(str(e)));pg.on('console',lambda m: errs.append(m.text) if m.type=='error' else None)
        await pg.add_init_script(INIT);await pg.goto(PAGE);await pg.wait_for_timeout(500)
        ready='!document.querySelector("#play").disabled&&!document.querySelector("#tSlow").disabled'
        meta='document.querySelector(".songhead .meta").textContent'
        await pg.click('#demo');await pg.wait_for_function(ready,timeout=240000);print('demo:',await pg.evaluate(meta))
        await pg.select_option('#style','bluegrass');await pg.wait_for_timeout(500);await pg.wait_for_function(ready,timeout=240000)
        print('bluegrass:',await pg.evaluate(meta),'| guitar',await pg.evaluate('document.querySelector("#gtr").value'),'| band',await pg.evaluate('JSON.stringify(ST.band)'))
        t0=await pg.evaluate('ST.song.tempo');await pg.click('#tFast');await pg.wait_for_timeout(500);await pg.wait_for_function(ready,timeout=240000)
        print('tempo',t0,'->',await pg.evaluate('ST.song.tempo'),await pg.evaluate('document.querySelector("#tempoVal").textContent'),'dur',await pg.evaluate('ST.buf.duration.toFixed(1)'))
        t=await pg.evaluate('Date.now()');await pg.select_option('#gtr','fingerpick');await pg.wait_for_timeout(300);await pg.wait_for_function(ready,timeout=120000)
        print('guitar change ms',await pg.evaluate('Date.now()')-t,'guitar',await pg.evaluate('ST.song.guitar'))
        await pg.select_option('#style','blues');await pg.wait_for_timeout(500);await pg.wait_for_function(ready,timeout=240000)
        await pg.fill('#mood','rent day');await pg.click('#write');await pg.wait_for_timeout(1500);await pg.wait_for_function(ready,timeout=240000)
        pr=await pg.evaluate('window.__prompt');print('write prompt has 12-bar:', '12-bar' in pr, '| blues style:', 'Delta and Piedmont blues' in pr)
        print('after write:',await pg.evaluate(meta),'| lead',await pg.evaluate('ST.song.breakLead'))
        await pg.screenshot(path='ref/ui_style.png')
        print('errors',errs[:6]);await b.close()
asyncio.run(main())
