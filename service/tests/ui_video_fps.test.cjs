const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');const vm=require('node:vm');const path=require('node:path');
const source=fs.readFileSync(path.join(__dirname,'../src/ui.html'),'utf8');
const shared=source.slice(source.indexOf('    function initialVideoFps('),source.indexOf('    async function loadManaged('));
const rendering=source.slice(source.indexOf('    function renderManaged('),source.indexOf('    function jstDay('));
const refresh=source.slice(source.indexOf('    function managedRefreshBlocked('),source.indexOf('    setInterval(refreshManaged'));
function fixture(saved,blockedStorage=false){
  let all=[],commands=[],stored=new Map(saved?[['picomado.youtube.fps',saved]]:[]),renderCount=0;
  const doc={activeElement:null,querySelector:()=>null};
  function el(tag,text,className){const n={tag,textContent:text||'',className,children:[],value:'',append(...v){this.children.push(...v)},reportValidity(){return all.filter(n=>n.tag==='select').every(n=>n.value!=='')}};all.push(n);return n}
  const context=vm.createContext({el,section:(...v)=>el('section',v[0]),button:text=>el('button',text),jobsPanel(){},bytesText:()=>'',
    textField(parent,label,type,value){const n=el('input',label);n.value=value||'';parent.append(n);return n},
    localStorage:{getItem(k){if(blockedStorage)throw Error('denied');return stored.get(k)},setItem(k,v){if(blockedStorage)throw Error('denied');stored.set(k,v)}},
    document:doc,deleteDialog:{open:false},busy:false,render(){renderCount++;context.showForm()},
    managedCommand:(id,command)=>commands.push(JSON.parse(JSON.stringify(command))),loadManaged:async()=>{},api:async()=>({plugins:{youtube_default_fps:30}})});
  vm.runInContext(shared+rendering+refresh+`;managedViews.youtube={data:{videos:[]},jobs:[]};let current={plugins:{youtube_default_fps:6}};globalThis.showForm=()=>renderManaged('youtube');globalThis.refresh=refreshManaged;`,context);
  function fields(){return {fps:all.find(n=>n.tag==='select'),url:all.find(n=>n.textContent==='YouTube URL'),form:all.find(n=>n.tag==='form')}}
  return {context,stored,commands,doc,get renderCount(){return renderCount},render(){all=[];context.showForm();return fields()},fields};
}
test('FPS has five explicit choices, required and no independent default UI',()=>{
  const f=fixture(),{fps}=f.render();assert.equal(fps.value,'12');assert.equal(fps.required,true);
  assert.deepEqual(fps.children.map(n=>n.value),[6,12,18,24,30]);assert.equal(source.includes('youtube_default_fps'),false);
});
test('selected FPS survives URL changes, rerender and is always sent numerically',()=>{
  const f=fixture('18');let x=f.render();x.fps.value='12';x.fps.onchange();x.url.value='https://youtu.be/abcdefghijk';x.url.oninput();
  x=f.render();assert.equal(x.fps.value,'12');assert.equal(x.url.value,'https://youtu.be/abcdefghijk');x.form.onsubmit({preventDefault(){}});
  assert.equal(f.commands[0].fps,12);assert.equal(f.stored.get('picomado.youtube.fps'),'12');
});
test('last browser selection is initial choice, invalid or inaccessible storage uses 12',()=>{
  assert.equal(fixture('24').render().fps.value,'24');assert.equal(fixture('7').render().fps.value,'12');assert.equal(fixture('30',true).render().fps.value,'12');
});
test('empty required FPS prevents conversion submission',()=>{
  const f=fixture(),x=f.render();x.fps.value='';x.form.onsubmit({preventDefault(){}});assert.equal(f.commands.length,0);
});
test('refresh rechecks focus after awaiting API and does not replace open FPS selection',async()=>{
  const f=fixture('18'),x=f.render();let resolve;const pending=new Promise(r=>resolve=r);f.context.loadManaged=()=>pending;
  const refresh=f.context.refresh();f.doc.activeElement={tagName:'SELECT'};x.fps.value='12';resolve();await refresh;
  assert.equal(f.renderCount,0);assert.equal(x.fps.value,'12');x.fps.onchange();f.doc.activeElement=null;
  await f.context.refresh();assert.equal(f.renderCount,1);assert.equal(f.fields().fps.value,'12');
});
test('periodic refresh and changed server settings preserve the selected conversion FPS',async()=>{
  const f=fixture(),x=f.render();x.fps.value='18';x.fps.onchange();await f.context.refresh();assert.equal(f.fields().fps.value,'18');
});
