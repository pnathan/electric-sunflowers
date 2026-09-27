import asyncio,os
ROOT=os.path.abspath(os.path.join(os.path.dirname(__file__),'..'))
PAGE='file://'+ROOT+'/dist/singer-songwriter-bot.html'
LAUNCH=dict(executable_path=os.environ['CHROME']) if os.environ.get('CHROME') else {}
from playwright.async_api import async_playwright
INIT="""
window.claude={use:async n=>{if(n!=='sample')return null;
 const f=async(p,o)=>{window.__prompt=p;window.__tier=o.modelTier;const j=JSON.stringify(DEMO_SONG).replace('Every Harbor','Mock Harbor');for(let i=200;i<j.length;i+=400){await new Promise(r=>setTimeout(r,60));o.onText({text:j.slice(0,i)});}return {text:j,truncated:false,modelTierApplied:'default'};};return f;}};
"""
async def main():
    async with async_playwright() as p:
        b=await p.chromium.launch(**LAUNCH,args=['--autoplay-policy=no-user-gesture-required'])
        pg=await b.new_page(viewport={'width':1280,'height':900})
        errs=[];pg.on('pageerror',lambda e: errs.append('PAGEERR '+str(e)));pg.on('console',lambda m: errs.append(m.text) if m.type=='error' else None)
        await pg.add_init_script(INIT)
        await pg.goto(PAGE)
        await pg.wait_for_timeout(500)
        await pg.fill('#mood','a test mood')
        await pg.click('#write')
        await pg.wait_for_timeout(700)
        await pg.screenshot(path='shot3.png')
        await pg.wait_for_function('!document.querySelector("#play").disabled',timeout=120000)
        await pg.click('input[value=soprano]',force=True)
        await pg.wait_for_function('document.querySelector(".meta").textContent.includes("soprano")',timeout=120000)
        await pg.wait_for_timeout(3000)
        print(await pg.evaluate('document.querySelector(".meta").textContent+" | "+document.querySelector("#time").textContent'))
        print(await pg.evaluate('window.__prompt.length'),await pg.evaluate('window.__tier'),await pg.evaluate('[...document.querySelectorAll(".songhead .meta")].map(e=>e.textContent).join(" / ")'))
        print(errs)
        await b.close()
asyncio.run(main())
