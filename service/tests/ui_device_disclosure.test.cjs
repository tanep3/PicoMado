// BT-AUDIO-20261107: real device rendering and refresh, with no device/network IO.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(process.env.PICOMADO_UI_SOURCE || `${__dirname}/../src/ui.html`, 'utf8');
function slice(start, end) { const a=source.indexOf(start),b=source.indexOf(end,a); assert(a>=0&&b>a); return source.slice(a,b); }
const helpers=slice('    function button(', '    function openDelete(');
const rendering=slice('    function render(){', '    Promise.all(');
const refresh=slice('    function managedRefreshBlocked(){', '    setInterval(refreshManaged,2000);');
const fields=slice('    const deviceFields=[', '    let current,');
const plain=value=>JSON.parse(JSON.stringify(value));
const text=node=>[node.textContent,...node.children.map(text)].filter(Boolean).join(' ');
function element(tag, value, className) {
  return {tag,textContent:value||'',className:className||'',children:[],dataset:{},attributes:{},open:false,disabled:false,
    append(...nodes){this.children.push(...nodes)},replaceChildren(...nodes){this.children=[...nodes]},
    setAttribute(key,value){this.attributes[key]=value},removeAttribute(key){delete this.attributes[key]},reportValidity(){return true},
    querySelectorAll(selector){
      const match=n=>{
        if(selector[0]==='.')return n.className.split(' ').includes(selector.slice(1));
        const tag=selector.match(/^[a-z][a-z0-9-]*/)?.[0];if(tag&&n.tag!==tag)return false;
        if(selector.includes('[open]')&&!n.open)return false;
        for(const data of selector.matchAll(/\[data-([a-z-]+)\]/g)) {
          const key=data[1].replace(/-([a-z])/g,(_,c)=>c.toUpperCase());if(!(key in n.dataset))return false;
        }
        return true;
      };
      return this.children.flatMap(n=>[...(match(n)?[n]:[]),...n.querySelectorAll(selector)]);
    },querySelector(selector){return this.querySelectorAll(selector)[0]||null}
  };
}
function device(id, incarnation=1) {
  return {id,incarnation,online:true,capabilities:['lcd'],selected:'clock',enabled:[],enabled_revision:2,
    pending:false,desired_revision:3,stored_revision:3,desired:{wifi_timeout_seconds:10,app_timeout_seconds:11,retry_initial_seconds:1,retry_max_seconds:20,heartbeat_seconds:5,offline_seconds:12,two_button_long_press_millis:900,unrelated:7},
    bluetooth:{revision:4,observed:'connected',desired:{enabled:true,volume:30,muted:false},candidates:[],pending:null}};
}
function fixture(devices=[device('one'),device('two')]) {
  const main=element('main'),calls=[],deletions=[],notices=[],page={};
  const f={main,calls,deletions,state:{order:[],order_revision:1,devices}};
  const sandbox={main,el:element,labels:{},current:f.state,busy:false,location:{pathname:'/'},deleteDialog:{open:false},
    document:{activeElement:{tagName:'BODY'},getElementById:id=>page[id]??=(element('div')),querySelectorAll:()=>[],querySelector:()=>null},
    loadManaged:async()=>{},api:async(path,method='GET',body)=>{calls.push({path,method,body:body===undefined?undefined:plain(body)});return f.state},
    run:async action=>{await action();return true},say:(...args)=>notices.push(args),openDelete:d=>deletions.push(d.id),renderPlugin:()=>{throw Error('home only')}
  };
  vm.createContext(sandbox);vm.runInContext(fields+helpers+rendering+refresh,sandbox);
  f.render=()=>{sandbox.current=f.state;sandbox.render()};f.refresh=()=>sandbox.refreshManaged();
  f.cards=()=>main.querySelectorAll('.device-card');
  f.card=id=>f.cards().find(c=>c.querySelector('h3').textContent===id);
  f.disclosure=(id,kind)=>f.card(id).querySelectorAll('details').find(d=>d.querySelector('summary')?.textContent===(kind==='settings'?'端末別の接続・操作設定':'端末の管理・削除'));
  f.bluetooth=id=>f.card(id).querySelector('[data-bluetooth]');
  f.render();return f;
}

test('Bluetooth stays visible and connection settings start collapsed below it',()=>{
  const f=fixture();
  for(const id of ['one','two']) {
    const card=f.card(id),bt=f.bluetooth(id),settings=f.disclosure(id,'settings'),management=f.disclosure(id,'management');
    assert.equal(bt.tag,'div');assert.equal(bt.querySelector('summary'),null);
    assert.match(text(bt),/Bluetoothスピーカー/);assert.equal(bt.querySelectorAll('input').length,3);
    assert(settings);assert.equal(settings.open,false);assert.equal(management.open,false);
    assert(card.children.indexOf(bt)<card.children.indexOf(settings));assert(card.children.indexOf(settings)<card.children.indexOf(management));
  }
  assert.equal(f.calls.length,0);
});

test('management remains open across repeated automatic refreshes without toggle-event timing',async()=>{
  const f=fixture();f.disclosure('one','management').open=true;
  for(let n=0;n<3;n++){await f.refresh();assert.equal(f.disclosure('one','management').open,true);assert.equal(f.disclosure('two','management').open,false)}
  assert(f.calls.every(call=>call.method==='GET'));assert.deepEqual(f.deletions,[]);
});

test('settings and management retain independent open and closed states per device',async()=>{
  const f=fixture();f.disclosure('one','settings').open=true;f.disclosure('two','management').open=true;
  await f.refresh();assert.equal(f.disclosure('one','settings').open,true);assert.equal(f.disclosure('one','management').open,false);assert.equal(f.disclosure('two','management').open,true);assert.equal(f.disclosure('two','settings').open,false);
  f.disclosure('one','settings').open=false;f.disclosure('one','management').open=true;
  await f.refresh();assert.equal(f.disclosure('one','settings').open,false);assert.equal(f.disclosure('one','management').open,true);assert.equal(f.disclosure('two','management').open,true);
});

test('device reorder preserves ownership and a replacement incarnation starts closed',async()=>{
  const f=fixture();f.disclosure('one','management').open=true;f.state.devices.reverse();await f.refresh();assert.equal(f.disclosure('one','management').open,true);assert.equal(f.disclosure('two','management').open,false);
  f.state.devices=f.state.devices.map(d=>d.id==='one'?device('one',2):d);await f.refresh();assert.equal(f.disclosure('one','management').open,false);
});

test('settings stay available on devices without Bluetooth metadata',()=>{
  const d=device('offline');d.online=false;d.bluetooth=null;const f=fixture([d]);assert.equal(f.bluetooth('offline'),null);assert.equal(f.disclosure('offline','settings').open,false);assert.equal(f.disclosure('offline','settings').querySelector('form').querySelectorAll('input').length,7);
});

test('settings save preserves target, revision, numeric fields and unrelated values',async()=>{
  const f=fixture();const form=f.disclosure('two','settings').querySelector('form');
  form.querySelectorAll('input').find(i=>i.dataset.key==='heartbeat_seconds').value='8';
  let prevented=false;await form.onsubmit({preventDefault(){prevented=true}});assert(prevented);
  assert.equal(f.calls.length,1);assert.deepEqual(f.calls[0],{path:'/api/devices/two/settings',method:'PUT',body:{incarnation:1,expected_revision:3,settings:{...f.state.devices[1].desired,heartbeat_seconds:8}}});
});

test('visible Bluetooth controls keep volume and mute commands device-specific',async()=>{
  const f=fixture();const inputs=f.bluetooth('two').querySelectorAll('input'),volume=inputs.find(i=>i.type==='range'),mute=inputs.find(i=>i.attributes['aria-label']==='two のミュート');
  volume.value='21';await volume.onchange();mute.checked=true;await mute.onchange();
  assert.deepEqual(f.calls.map(c=>c.body),[{incarnation:1,expected_revision:4,command:{action:'set_volume',volume:21}},{incarnation:1,expected_revision:4,command:{action:'set_muted',muted:true}}]);assert(f.calls.every(c=>c.path==='/api/devices/two/bluetooth'));
});

test('opening management never deletes and the delete button still opens confirmation',async()=>{
  const f=fixture();f.disclosure('two','management').open=true;await f.refresh();
  assert.deepEqual(f.deletions,[]);f.disclosure('two','management').querySelector('button').onclick();assert.deepEqual(f.deletions,['two']);assert(f.calls.every(c=>c.method==='GET'));
});
