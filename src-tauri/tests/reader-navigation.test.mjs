import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';
import assert from 'node:assert/strict';
const script=readFileSync(new URL('../src/reader_navigation.js',import.meta.url),'utf8');
const node=(rect,style={})=>({parentElement:null,getClientRects:()=>[{}],getBoundingClientRect:()=>rect,style});
const rect=(left,top,width,height)=>({left,top,width,height,right:left+width,bottom:top+height});
const marker=(text, bounds=rect(0,950,678,30), attributes={})=>Object.assign(node(bounds), {textContent:text,getAttribute:name=>attributes[name]??null});
const control=(bounds,style={})=>Object.assign(node(bounds,style),{localName:'button'});
const bar=(bounds=rect(0,0,678,62))=>Object.assign(node(bounds),{querySelectorAll:()=>[control(bounds)]});
function inspect({header=bar(),toolbar=null,toc=null,menu=null,text='位置1/196 ● 0%',markers=[marker(text)]}={}) {
  return JSON.parse(JSON.stringify(runInNewContext(script,{
    innerWidth:678,innerHeight:997,
    getComputedStyle:e=>({display:'block',visibility:'visible',opacity:'1',overflowX:'visible',overflowY:'visible',...e.style}),
    document:{getElementById:id=>id==='reader-header'?header:id==='top-menu-bar'?toolbar:{},querySelector:selector=>selector.includes('side-menu-close')?toc:selector==='ion-menu.side-menu.show-menu'?menu:{},querySelectorAll:()=>markers},
  })));
}
test('reads the reader position indicator',()=>{
  const ui=inspect();
  assert.equal(ui.position,1);
  assert.equal(ui.total,196);
  assert.equal(ui.chromeVisible,true);
  assert.equal(ui.tocOpen,false);
});
test('uses a complete footer even when the first header indicator has no total',()=>{
  const ui=inspect({markers:[marker('位置1'),marker('位置1 / 196 ● 0%')]});
  assert.equal(ui.position,1);
  assert.equal(ui.total,196);
});
test('supports prefix-free fractions only inside the known position UI',()=>{
  for(const text of ['1 / 196 ● 0%','１／１９６','1 of 196','位置: 1 / 196','Page: 1 of 196']) {
    assert.equal(inspect({text}).position,1);
    assert.equal(inspect({text}).total,196);
  }
});
test('can read an accessible position label when the element text is empty',()=>{
  const ui=inspect({markers:[marker('',rect(0,950,678,30),{'aria-label':'Page 1 of 196'})]});
  assert.equal(ui.position,1);
  assert.equal(ui.total,196);
});
test('ignores an empty first marker and stale offscreen indicators',()=>{
  const ui=inspect({markers:[marker(''),marker('位置9/196',rect(0,-60,678,30)),marker('位置1/196')]});
  assert.equal(ui.position,1);
  assert.equal(ui.total,196);
});
test('does not combine a visible current position with an offscreen total',()=>{
  const ui=inspect({markers:[marker('位置1'),marker('位置1/196',rect(0,-60,678,30))]});
  assert.equal(ui.position,1);
  assert.equal(ui.total,null);
});
test('fails closed for conflicting visible positions or missing indicators',()=>{
  for(const markers of [[marker('位置1/196'),marker('位置2/196')],[]]) {
    assert.equal(inspect({markers}).position,null);
    assert.equal(inspect({markers}).total,null);
  }
});
test('reports bounded indicator text for diagnosing an unsupported format',()=>{
  assert.deepEqual(inspect({text:'位置: 1'}).positionTexts,['位置: 1']);
  assert.equal(inspect({text:'x'.repeat(1000)}).positionTexts[0].length,120);
});
test('recognizes a closed Ionic panel moved beyond the right edge',()=>{
  assert.equal(inspect({toc:node(rect(800,72,54,54))}).tocOpen,false);
  assert.equal(inspect({toc:node(rect(218,72,54,54))}).tocOpen,true);
});
test('a closing TOC remains open while its backdrop can intercept the center click',()=>{
  const toc=node(rect(1000,72,54,54));
  const menu=node(rect(0,0,678,997));
  const closing=inspect({toc,menu});
  assert.equal(closing.tocCloseVisible,false);
  assert.equal(closing.tocOpen,true);
  assert.equal(inspect({toc,menu:null}).tocOpen,false);
  menu.style.display='none';
  assert.equal(inspect({toc,menu}).tocOpen,false);
});
test('recognizes a hidden toolbar through its CSS ancestor',()=>{
  const header=bar();
  header.parentElement=node(rect(0,0,678,997),{opacity:'0'});
  const button=control(rect(520,5,44,44));
  button.parentElement=header;
  header.querySelectorAll=()=>[button];
  assert.equal(inspect({header}).chromeVisible,false);
});
test('does not infer the first page from a missing or later position',()=>{
  assert.equal(inspect({text:'位置4/196 ● 2%'}).position,4);
  assert.equal(inspect({text:''}).position,null);
});
test('distinguishes a stationary header host from an offscreen toolbar',()=>{
  assert.equal(inspect({toolbar:bar(rect(0,-61,678,60))}).chromeVisible,false);
  assert.equal(inspect({toolbar:bar(rect(0,1,678,60))}).chromeVisible,true);
});
test('a stationary header without top-menu-bar uses the actual Ionic button visibility',()=>{
  const header=node(rect(0,0,678,62));
  const hiddenButton=node(rect(520,-53,44,44));
  const host=node(rect(520,-53,44,44));
  header.querySelectorAll=()=>[host,hiddenButton];
  assert.equal(inspect({header}).chromeVisible,false);
  host.getBoundingClientRect=()=>rect(520,5,44,44);
  hiddenButton.getBoundingClientRect=()=>rect(520,5,44,44);
  assert.equal(inspect({header}).chromeVisible,true);
});
test('a visible toolbar container with hidden buttons is not mistaken for open chrome',()=>{
  const toolbar=node(rect(0,0,678,62));
  const button=node(rect(520,5,44,44),{visibility:'hidden'});
  toolbar.querySelectorAll=()=>[button];
  assert.equal(inspect({toolbar}).chromeVisible,false);
  button.style.visibility='visible';
  assert.equal(inspect({toolbar}).chromeVisible,true);
});
test('an empty stationary header is not evidence of visible controls',()=>{
  const ui=inspect({header:node(rect(0,0,678,62))});
  assert.equal(ui.chromeVisible,false);
  assert.deepEqual(ui.toolbar,{controlCount:0,visibleControlCount:0,hostVisible:true});
});
test('toolbar controls clipped by an ancestor are not visible',()=>{
  for(const [bounds,style] of [
    [rect(0,0,678,0),{overflowY:'hidden'}],
    [rect(0,0,0,62),{overflowX:'clip'}],
    [rect(0,-62,678,62),{overflowY:'auto'}],
    [rect(0,0,678,62),{contentVisibility:'hidden'}],
  ]) {
    const header=node(bounds,style);
    const button=control(rect(520,5,44,44));
    button.parentElement=header;
    header.querySelectorAll=()=>[button];
    assert.equal(inspect({header}).chromeVisible,false);
  }
});
test('clipping is intersected across ancestors without rejecting a visible portion',()=>{
  const header=node(rect(0,0,678,62),{overflowY:'hidden'});
  const parent=node(rect(0,40,678,957),{overflowY:'hidden'});
  header.parentElement=parent;
  const button=control(rect(520,5,44,30));
  button.parentElement=header;
  header.querySelectorAll=()=>[button];
  assert.equal(inspect({header}).chromeVisible,false);
  button.getBoundingClientRect=()=>rect(520,30,44,30);
  assert.equal(inspect({header}).chromeVisible,true);
});
test('Ionic hosts use their rendered shadow buttons rather than the host rectangle',()=>{
  const header=node(rect(0,0,678,62));
  const host=Object.assign(node(rect(520,5,44,44)),{localName:'ion-button'});
  const button=control(rect(520,-53,44,44));
  button.getRootNode=()=>({host});
  host.parentElement=header;
  host.shadowRoot={querySelectorAll:()=>[button]};
  header.querySelectorAll=selector=>selector==='*'?[host]:[host];
  assert.equal(inspect({header}).chromeVisible,false);
  assert.deepEqual(inspect({header}).toolbar,{controlCount:1,visibleControlCount:0,hostVisible:true});
  button.getBoundingClientRect=()=>rect(520,5,44,44);
  assert.equal(inspect({header}).chromeVisible,true);
  host.style.opacity='0';
  assert.equal(inspect({header}).chromeVisible,false);
});
test('finds controls inside a toolbar shadow root',()=>{
  const header=node(rect(0,0,678,62));
  const button=control(rect(520,5,44,44));
  button.getRootNode=()=>({host:header});
  header.shadowRoot={querySelectorAll:selector=>selector==='*'?[]:[button]};
  header.querySelectorAll=()=>[];
  assert.equal(inspect({header}).chromeVisible,true);
  button.style.display='none';
  assert.equal(inspect({header}).chromeVisible,false);
});
test('reads spread ranges as a start-end pair',()=>{
  for(const text of ['位置3-4/196','位置 3～4 / 196','3-4 of 196','Page 3–4 of 196']) {
    const ui=inspect({text});
    assert.equal(ui.position,3);
    assert.equal(ui.positionEnd,4);
    assert.equal(ui.total,196);
  }
  const single=inspect({text:'位置3/196'});
  assert.equal(single.position,3);
  assert.equal(single.positionEnd,null);
  assert.equal(inspect({text:'位置4-2/196'}).total,null);
});
test('parses localized page totals without using rounded percentages as the end',()=>{
  for(const text of ['ページ 196 / 196 ● 100%', 'Page 196 of 196', '位置１９６／１９６']) {
    assert.equal(inspect({text}).position,196);
    assert.equal(inspect({text}).total,196);
  }
  assert.equal(inspect({text:'Location 1,234 / 2,000'}).total,2000);
  assert.equal(inspect({text:'位置195/196 ● 100%'}).position,195);
  for(const text of ['', '100%', '位置0/196', '位置197/196', '位置1/0', '位置1', '位置4294967296/4294967296']) {
    assert.equal(inspect({text}).total,null);
  }
});
