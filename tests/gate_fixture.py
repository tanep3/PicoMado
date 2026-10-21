"""Private test repository. Its approvals/receipts are fixtures, never production claims."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

SOURCE=Path(__file__).resolve().parents[1]


class GateFixture:
    def __init__(self,root):
        self.root=Path(root)
        for name in ['tools/project_gate.py','tools/build_lineage.py','development/standard/v1/workflow.py','firmware/tools/flash.sh']:
            p=self.root/name;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(SOURCE/name,p)
        (self.root/'.gitignore').write_text('/.gate/\n/sdk/\n/firmware/build/\n/firmware/devices/*/config.h\n/firmware/devices/*/board-id.txt\n__pycache__/\n')
        self.put('contracts/development-context.json',dict(active_change='FIXTURE'))
        (self.root/'contracts/design.md').write_text('Fixed fixture contract\n')
        self.case=dict(id='FIXTURE',owner='parent fixture',implementer='fixture-worker',status='in_progress',design_artifacts=['contracts/design.md'],
            stages={n:dict(state='not_started',evidence=[])for n in ['design','implementation','independent_verification','operational_application','effectiveness_confirmation']})
        self.save_case();subprocess.run(['git','init','-q',str(self.root)],check=True);self.commit()
        first=self.git('rev-parse','HEAD');self.case['design_commit']=first;self.case['stages']['design']=dict(state='verified',evidence=[dict(kind='commit',commit=first)]);self.save_case();self.commit()
        module=importlib.util.spec_from_file_location('fixture_gate_'+str(id(self)),self.root/'tools/project_gate.py')
        self.gate=importlib.util.module_from_spec(module);sys.modules[module.name]=self.gate;module.loader.exec_module(self.gate)

    def git(self,*args):return subprocess.check_output(['git','-C',str(self.root),*args],text=True).strip()
    def commit(self):
        self.git('add','.');self.git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-qm','fixture state')
    def put(self,name,value):
        p=self.root/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(value,indent=2)+'\n');return p
    def save_case(self):self.put('docs/changes/FIXTURE.json',self.case)
    def cli(self,*args):
        return subprocess.run([sys.executable,str(self.root/'tools/project_gate.py'),*map(str,args)],cwd=self.root,capture_output=True,text=True,env=dict(os.environ,PYTHONDONTWRITEBYTECODE='1'))

    def check_fixture(self):
        # These are metadata parser fixtures. The real repository separately runs all three checks.
        checks=[]
        for name in ['structural','usb-domain','rust']:
            p=self.root/'.gate'/(name+'.log');p.parent.mkdir(exist_ok=True);p.write_text('FIXTURE CHECK METADATA ONLY\n')
            checks.append(dict(name=name,exit=0,log=str(p),SHA256=self.gate.sha(p)))
        snap=self.gate.input_snapshot();self.report=self.put('.gate/check.json',dict(schema=2,change='FIXTURE',stage='implementation',complete=True,committed=True,source_commit=self.git('rev-parse','HEAD'),checks=checks,**snap))
        return self.report

    def implementation_verified(self):
        self.case['stages']['implementation']=dict(state='verified',evidence=[dict(kind='check',path=str(self.report),SHA256=self.gate.sha(self.report))]);self.save_case();self.commit()

    def parent_reviewed(self):
        proof=self.put('.gate/independent.json',dict(change='FIXTURE',stage='independent_verification',subject_digest=self.gate.input_snapshot()['subject_digest'],owner='parent',reviewer='FIXTURE ONLY',accepted=True))
        self.case['stages']['independent_verification']=dict(state='verified',evidence=[dict(kind='review',path=str(proof),SHA256=self.gate.sha(proof))]);self.save_case();self.commit()

    def native_server(self):
        p=self.root/'service';(p/'src').mkdir(parents=True)
        (p/'Cargo.toml').write_text('[package]\nname="fixture-server"\nversion="0.1.0"\nedition="2021"\n[features]\ndefault=[]\nobserve=[]\n[[bin]]\nname="picomado-server"\npath="src/main.rs"\n')
        (p/'src/main.rs').write_text('fn main(){println!("fixture; never deployed");}\n')
        subprocess.run(['cargo','generate-lockfile','--offline','--manifest-path',str(p/'Cargo.toml')],check=True,capture_output=True)
        self.commit()

    def native_firmware_container(self):
        sdk=self.root/'sdk';sdk.mkdir();(sdk/'fixture.h').write_text('#define FIXTURE_VALUE 42\n')
        subprocess.run(['git','init','-q',str(sdk)],check=True)
        subprocess.run(['git','-C',str(sdk),'add','.'],check=True)
        subprocess.run(['git','-C',str(sdk),'-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-qm','fixture SDK'],check=True)
        fw=self.root/'firmware';(fw/'devices/fixture').mkdir(parents=True)
        (fw/'devices/fixture/config.h').write_text('/* fixture binding */\n');(fw/'devices/fixture/board-id.txt').write_text('0000000000000001\n')
        (fw/'main.c').write_text('#include "fixture.h"\nint main(void){return FIXTURE_VALUE;}\n')
        (fw/'container.py').write_text('import struct,sys\nfrom pathlib import Path\nb=bytearray(512);struct.pack_into("<8I",b,0,0x0a324655,0x9e5d5157,0x2000,0x10000100,256,0,1,0xe48bff56);b[32:288]=Path(sys.argv[1]).read_bytes()[:256];struct.pack_into("<I",b,508,0x0ab16f30);Path(sys.argv[2]).write_bytes(b)\n')
        (fw/'CMakeLists.txt').write_text('cmake_minimum_required(VERSION 3.16)\nproject(fixture C)\nset(PICO_SDK_PATH "${CMAKE_CURRENT_LIST_DIR}/../sdk" CACHE PATH "fixture SDK")\nadd_executable(program main.c)\ntarget_include_directories(program PRIVATE ${PICO_SDK_PATH})\nadd_custom_command(OUTPUT ${CMAKE_BINARY_DIR}/fixture.uf2 COMMAND python3 ${CMAKE_CURRENT_SOURCE_DIR}/container.py $<TARGET_FILE:program> ${CMAKE_BINARY_DIR}/fixture.uf2 DEPENDS program container.py)\nadd_custom_target(container ALL DEPENDS ${CMAKE_BINARY_DIR}/fixture.uf2)\n')
        self.commit()

    def build(self,kind):
        args=['build','--kind',kind,'--build-dir',self.root/'.gate/build','--report',self.report]
        if kind=='firmware':args+=['--device','fixture']
        result=self.cli(*args)
        if result.returncode:raise AssertionError(result.stderr)
        value=json.loads(result.stdout);self.image=Path(value['artifact']);self.build_receipt=Path(value['build_receipt']);return value

    def record(self,kind):
        args=['record','--report',self.report,'--build-receipt',self.build_receipt,'--artifact',self.image,'--kind',kind,'--out',self.root/'.gate/build/candidate.json']
        if kind=='firmware':args+=['--device','fixture','--board-id',self.root/'firmware/devices/fixture/board-id.txt','--config',self.root/'firmware/devices/fixture/config.h']
        p=self.cli(*args)
        if p.returncode:raise AssertionError(p.stderr)
        self.candidate=self.root/'.gate/build/candidate.json'
        self.approval=self.put('.gate/build/review.json',dict(owner='parent',reviewer='FIXTURE ONLY',accepted=True,change='FIXTURE',subject_digest=self.gate.input_snapshot()['subject_digest'],artifact_SHA256=self.gate.sha(self.image),candidate_SHA256=self.gate.sha(self.candidate),build_receipt_SHA256=self.gate.sha(self.build_receipt)))

    def preflight(self,kind):
        args=['preflight','--candidate',self.candidate,'--review',self.approval,'--artifact',self.image]
        if kind=='firmware':args+=['--device','fixture','--board-id',self.root/'firmware/devices/fixture/board-id.txt','--config',self.root/'firmware/devices/fixture/config.h']
        return self.cli(*args)
