const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const source=fs.readFileSync(path.join(__dirname,'../src/ui.html'),'utf8');
const code=source.slice(source.indexOf('    const thumbnailImages='),source.indexOf('    async function loadManaged('));
function fixture(){
  const nodes=[],commands=[];let release;
  const view={jobs:[],data:{thumbnail_pending:[]}},gate=new Promise(r=>release=r);
  function el(tag,text,className){
    const n={tag,textContent:text||'',className,children:[],style:{},srcWrites:0,
      append(...items){for(const item of items){item.remove?.();this.children.push(item);item.parent=this}},
      remove(){if(this.parent){this.parent.children=this.parent.children.filter(n=>n!==this);this.parent=null}}};
    Object.defineProperty(n,'src',{set(v){this._src=v;this.srcWrites++},get(){return this._src}});nodes.push(n);return n;
  }
  const context=vm.createContext({el,button:(text,kind)=>el('button',text,kind),managedViews:{youtube:view},managedCommand:async(id,command)=>{commands.push({id,...JSON.parse(JSON.stringify(command))});await gate;return true}});
  vm.runInContext(code+';globalThis.show=renderYoutubeThumbnail',context);
  const video={metadata:{id:'abcdefghijk',title:'Test clip',thumbnail:null}};
  return {view,commands,release,video,render(){const parent=el('article');context.show(video,parent);const box=parent.children[0];return {box,image:box.children.find(n=>n.tag==='img'),button:box.children.flatMap(n=>n.children).find(n=>n.tag==='button'),texts:box.children.flatMap(n=>n.children).map(n=>n.textContent)}}};
}
test('missing thumbnail offers a dedicated request, blocks double-clicks, and allows another explicit attempt',async()=>{
  const f=fixture(),{button}=f.render();assert.equal(button.textContent,'サムネ再取得');
  const pending=button.onclick();await button.onclick();assert.equal(button.disabled,true);assert.equal(button.textContent,'取得中…');
  assert.deepEqual(f.commands,[{id:'youtube',kind:'retry_thumbnail',video_id:'abcdefghijk'}]);f.release();await pending;
  assert.equal(button.disabled,false);assert.equal(button.textContent,'サムネ再取得');
});
test('server pending disables retry and a completed failure displays its safe reason',()=>{
  const f=fixture();f.view.data.thumbnail_pending=['abcdefghijk'];assert.equal(f.render().button.disabled,true);
  f.view.data.thumbnail_pending=[];f.view.jobs=[{token:{slot:'thumbnail_abcdefghijk'},status:'failed',failure:{message:'サムネを取得できませんでした（HTTP 404）'}}];
  const result=f.render();assert.equal(result.button.disabled,false);assert.equal(result.texts.some(t=>t.includes('HTTP 404')),true);
});
test('the same thumbnail node and source survive repeated list renders; changed content gets a new image',()=>{
  const f=fixture();f.video.metadata.thumbnail='thumbnail-abcdefghijk-111111111111111111111111';
  const first=f.render(),second=f.render();assert.equal(second.image,first.image);assert.equal(first.image.srcWrites,1);assert.equal(second.button,undefined);
  f.video.metadata.thumbnail='thumbnail-abcdefghijk-222222222222222222222222';assert.notEqual(f.render().image,first.image);
});
test('a broken saved image exposes retry without replacing the rest of the form',()=>{
  const f=fixture();f.video.metadata.thumbnail='thumbnail-abcdefghijk';const initial=f.render();initial.image.onerror();
  assert.equal(initial.image.style.display,'none');const result=f.render();assert.equal(result.image,initial.image);assert.equal(result.button.textContent,'サムネ再取得');
});
test('successful repair of identical image bytes reloads a previously broken URL once',()=>{
  const f=fixture();f.video.metadata.thumbnail='thumbnail-abcdefghijk';const old=f.render().image;old.onerror();
  f.view.jobs=[{token:{slot:'thumbnail_abcdefghijk',generation:42},status:'succeeded'}];
  const repaired=f.render();assert.notEqual(repaired.image,old);assert.equal(repaired.button,undefined);assert.equal(repaired.image.src.endsWith('?v=42'),true);
  assert.equal(f.render().image,repaired.image);assert.equal(repaired.image.srcWrites,1);
});
