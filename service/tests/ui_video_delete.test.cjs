const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const source=fs.readFileSync(process.env.PICOMADO_UI_SOURCE||path.join(__dirname,'../src/ui.html'),'utf8');
const managed=source.slice(source.indexOf('    async function managedCommand('),source.indexOf('    function textField('));
const deletion=source.slice(source.indexOf('    async function deleteVideo('),source.indexOf('    function renderManaged('));
const plain=v=>JSON.parse(JSON.stringify(v));
function fixture(options={}){
  const all=[],calls=[],commands=[],notices=[];
  const view={revision:1,jobs:[],data:{job:options.job||null,active:options.active?{conversion:{video_id:options.active,key:options.activeKey}}:null}};
  let cancelPolls=0;
  function element(tag,text,className){
    const n={tag,textContent:text||'',className,children:[],disabled:false,open:false,removed:false,attributes:{},
      append(...children){this.children.push(...children)},setAttribute(k,v){this.attributes[k]=v},
      showModal(){this.open=true},close(){this.open=false;this.onclose?.()},remove(){this.removed=true},focus(){this.focused=true}};
    all.push(n);return n;
  }
  const body=element('body');
  async function api(route,method='GET',body){
    calls.push({route,method,body});
    await options.beforeApi?.(route,method,body);
    if(method==='POST'){
      const command=plain(body.command);commands.push(command);
      if(options.failCommand===command.kind)throw new Error('test command failure');
      if(command.kind==='cancel'){view.data.job.status='Cancelling';cancelPolls=2}
      if(command.kind==='stop')view.data.active=null;
      view.revision++;return structuredClone(view);
    }
    if(route==='/api/plugins/youtube'){
      if(cancelPolls&&!--cancelPolls)view.data.job.status='Ready';
      return structuredClone(view);
    }
    return {};
  }
  const context=vm.createContext({document:{body},el:element,button:(text,kind)=>element('button',text,kind),api,
    say:(message,error)=>notices.push({message,error}),render(){},setTimeout:callback=>queueMicrotask(callback)});
  vm.runInContext('let busy=false,current;const managedViews={youtube:{revision:1}};'+managed+deletion+';globalThis.openDelete=deleteVideo;',context);
  return {all,calls,commands,notices,view,options,async open(video={metadata:{id:'abcdefghijk',title:'A <named> clip'}},target){
    await context.openDelete(video,target);const dialog=body.children.at(-1);const buttons=dialog.children.find(n=>n.tag==='div').children;
    return {dialog,cancel:buttons.find(b=>b.textContent==='キャンセル'),confirm:buttons.find(b=>b.textContent==='削除する')};
  }};
}
test('video title and two buttons are shown without requiring ID input',async()=>{
  const f=fixture(),{dialog,cancel,confirm}=await f.open();
  assert.equal(f.all.some(n=>n.tag==='input'),false);
  assert.equal(dialog.children.find(n=>n.tag==='p').textContent.includes('A <named> clip'),true);
  assert.equal(confirm.disabled,false);assert.equal(cancel.focused,true);assert.equal(f.calls.length,0);
});
test('cancel and a stale confirm callback send no requests',async()=>{
  const f=fixture(),{dialog,cancel,confirm}=await f.open();cancel.onclick();await confirm.onclick();
  assert.equal(dialog.open,false);assert.equal(dialog.removed,true);assert.equal(f.calls.length,0);
});
test('confirmed delete captures the intended ID and sends internal confirmation',async()=>{
  const f=fixture(),video={metadata:{id:'abcdefghijk',title:'Clip'}},{dialog,confirm}=await f.open(video);
  video.metadata.id='another-id';await confirm.onclick();
  assert.deepEqual(f.commands,[{kind:'delete',video_id:'abcdefghijk',confirmation:'abcdefghijk'}]);assert.equal(dialog.removed,true);
});
test('duplicate clicks and escape are blocked while confirmation is being processed',async()=>{
  let release;const pending=new Promise(resolve=>release=resolve);let blocked=false;
  const f=fixture({beforeApi:async(route,method)=>{if(!blocked&&method==='GET'){blocked=true;await pending}}});
  const {dialog,cancel,confirm}=await f.open();const first=confirm.onclick();await confirm.onclick();
  assert.equal(confirm.disabled,true);assert.equal(cancel.disabled,true);
  let prevented=false;dialog.oncancel({preventDefault(){prevented=true}});cancel.onclick();assert.equal(prevented,true);assert.equal(dialog.open,true);
  release();await first;assert.equal(f.commands.length,1);
});
test('working target is cancelled, awaited, stopped and deleted in order',async()=>{
  const f=fixture({job:{video_id:'abcdefghijk',status:'Working'},active:'abcdefghijk'}),{confirm}=await f.open();await confirm.onclick();
  assert.deepEqual(f.commands.map(c=>c.kind),['cancel','stop','delete']);assert.equal(f.commands.at(-1).video_id,'abcdefghijk');
});
test('another video job and active playback are not cancelled or stopped',async()=>{
  const f=fixture({job:{video_id:'other-video',status:'Working'},active:'other-video'}),{confirm}=await f.open();await confirm.onclick();
  assert.deepEqual(f.commands.map(c=>c.kind),['delete']);
});
test('read error remains visible and a deliberate retry is possible',async()=>{
  let fail=true;const f=fixture({beforeApi:()=>{if(fail)throw new Error('test read failure')}}),{dialog,cancel,confirm}=await f.open();
  await confirm.onclick();assert.equal(dialog.open,true);assert.equal(confirm.disabled,false);assert.equal(cancel.disabled,false);
  assert.deepEqual(f.notices.at(-1),{message:'test read failure',error:true});assert.equal(f.commands.length,0);
  fail=false;await confirm.onclick();assert.equal(f.commands.length,1);assert.equal(dialog.removed,true);
});
test('delete error keeps dialog open and preserves error before retry',async()=>{
  const f=fixture({failCommand:'delete'}),{dialog,confirm}=await f.open();await confirm.onclick();
  assert.equal(dialog.open,true);assert.equal(confirm.disabled,false);assert.equal(f.notices.at(-1).error,true);
  f.options.failCommand=null;await confirm.onclick();assert.equal(dialog.removed,true);assert.equal(f.commands.length,2);
});
test('failed cancellation never proceeds to stop or delete',async()=>{
  const f=fixture({job:{video_id:'abcdefghijk',status:'Working'},active:'abcdefghijk',failCommand:'cancel'}),{dialog,confirm}=await f.open();
  await confirm.onclick();assert.equal(dialog.open,true);assert.equal(confirm.disabled,false);assert.deepEqual(f.commands.map(c=>c.kind),['cancel']);
});

const sourceTarget={kind:'source',key:'111111111111111111111111',full:true};
const variantTarget={kind:'conversion',key:'222222222222222222222222',fps:12,clip:{start_ms:12000,end_ms:184000}};
test('source confirmation names original and does not stop converted playback',async()=>{
  const f=fixture({active:'abcdefghijk'}),{dialog,confirm}=await f.open(undefined,sourceTarget);await confirm.onclick();
  assert.match(dialog.children[0].textContent,/原本/);assert.deepEqual(f.commands,[{kind:'delete',video_id:'abcdefghijk',confirmation:'abcdefghijk',target:'source',key:sourceTarget.key}]);
});
test('variant dialog names FPS, exact range and format; only that key is sent',async()=>{
  const f=fixture({active:'abcdefghijk',activeKey:'other-key'}),{dialog,confirm}=await f.open(undefined,variantTarget);await confirm.onclick();
  assert.match(dialog.children[0].textContent,/12 fps.*12.0–184.0 秒.*160×80/);
  assert.deepEqual(f.commands,[{kind:'delete',video_id:'abcdefghijk',confirmation:'abcdefghijk',target:'conversion',key:variantTarget.key}]);
});
test('active selected variant is stopped before its scoped deletion',async()=>{
  const f=fixture({active:'abcdefghijk',activeKey:variantTarget.key}),{confirm}=await f.open(undefined,variantTarget);await confirm.onclick();
  assert.deepEqual(f.commands.map(c=>c.kind),['stop','delete']);
});
test('scoped cancellation sends nothing and does not stop any playback',async()=>{
  for(const target of [sourceTarget,variantTarget]){const f=fixture({active:'abcdefghijk'}),{cancel,confirm}=await f.open(undefined,target);cancel.onclick();await confirm.onclick();assert.equal(f.calls.length,0)}
});
