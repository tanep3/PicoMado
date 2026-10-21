#!/usr/bin/env python3
"""Project projection: data table to exhaustive real WireMessage identity."""
import argparse,json,subprocess,sys,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--check',action='store_true');a=p.parse_args()
subprocess.run([sys.executable,str(ROOT/'development/standard/v1/generate.py'),'--root',str(ROOT)]+(['--check']if a.check else[]),check=True)
s=json.loads((ROOT/'contracts/observation-domain.json').read_text())['rust_identity']
actual=(ROOT/'service/src/protocol.rs').read_text().split('pub enum WireMessage {',1)[1].split('impl WireMessage',1)[0]
variants=set(re.findall(r'^    (\w+) \{',actual,re.M));declared=[v for group in ['token','session','sessionless']for v in s[group]]
if len(declared)!=len(set(declared))or set(declared)!=variants:raise SystemExit('identity model must classify every real WireMessage variant exactly once')
lines=['// Generated from the project observation model. No wildcard fallback.', 'use crate::protocol::WireMessage;', 'pub fn identity(message:&WireMessage)->(u64,u64,u8){', ' let(s,f)=match message{']
for key,body in [('token','(token.session_epoch,token.frame_id)'),('session','(*session_epoch,0)'),('sessionless','(0,0)')]:
 pattern='token,..'if key=='token'else'session_epoch,..'if key=='session'else'..'
 lines += [' | '.join('WireMessage::'+v+'{'+pattern+'}'for v in s[key])+'=>'+body+',']
lines+=[' };(s,f,message.kind())','}']
path=ROOT/'service/src/observation_identity.rs';text='\n'.join(lines)+'\n'
if a.check:
 if not path.exists()or path.read_text()!=text:raise SystemExit('generated identity drift')
else:path.write_text(text)
