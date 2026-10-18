const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const source=fs.readFileSync(path.join(__dirname,'../src/ui.html'),'utf8');
const code=source.slice(source.indexOf('    function renderClock(){'),source.indexOf('    function renderPlugin(id){'));
function fixture(supported=false){
  const nodes=[],calls=[];
  function el(tag,text,className){const n={tag,textContent:text||'',className,children:[],value:'',append(...c){this.children.push(...c)},reportValidity(){return true}};nodes.push(n);return n;}
  const current={devices:[{id:'LCD',capabilities:['lcd'],incarnation:3,plugin_configs:{clock:{revision:1,full_refresh_minutes:10}}},{id:'E-ink_1',capabilities:supported?['eink','eink-clock-v1']:['eink'],incarnation:5,plugin_configs:{clock:{revision:7,full_refresh_minutes:15}}}]};
  const ctx=vm.createContext({current,document:{getElementById:()=>({})},el,section:(title,description)=>el('section',description),button:text=>el('button',text),
    field(parent,key,label,value){const n=el('input',label);n.value=value;parent.append(n);return n;},
    api:async(...args)=>{calls.push(args);return current;},run:async action=>{await action();return true;}});
  vm.runInContext(code+';renderClock();',ctx);return {nodes,calls};
}
test('clock interval is displayed only for E-ink, with bounds and truthful firmware status',()=>{
  const {nodes}=fixture();const inputs=nodes.filter(n=>n.tag==='input');assert.equal(inputs.length,1);assert.equal(inputs[0].value,15);assert.equal(inputs[0].min,'1');assert.equal(inputs[0].max,'60');
  assert(nodes.some(n=>n.textContent.includes('現行ファームでは毎分フラッシュ')));
  assert(source.includes('/plugins/clock'));assert(source.includes("clock:'時計'"));
});
test('saving interval sends exact device, incarnation, revision and numeric minutes',async()=>{
  const {nodes,calls}=fixture();nodes.find(n=>n.tag==='input').value='20';
  await nodes.find(n=>n.tag==='form').onsubmit({preventDefault(){}});
  assert.equal(calls[0][0],'/api/devices/E-ink_1/plugin-config/clock');
  assert.equal(JSON.stringify(calls[0][2]),JSON.stringify({incarnation:5,expected_revision:7,config:{full_refresh_minutes:20}}));
});

test('new E-ink capability reports support without the old firmware warning',()=>{
  const {nodes}=fixture(true);assert(nodes.some(n=>n.textContent.includes('部分更新対応ファーム（次の時計画像から適用）')));
  assert(!nodes.some(n=>n.textContent.includes('旧ファーム：')));
});
