"""Project build adapter. Only successful controlled builds issue lineage receipts."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import tomllib


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def require(ok,text):
    if not ok:raise ValueError(text)
def git(path,*args):return subprocess.check_output(['git','-C',str(path),*args],text=True).strip()
def tool(path):
    resolved=Path(shutil.which(str(path))or path).absolute();require(resolved.is_file(),'build tool missing')
    p=subprocess.run([str(resolved),'--version'],capture_output=True,timeout=10,check=True)
    return dict(path=str(resolved),resolved_path=str(resolved.resolve()),SHA256=sha(resolved),version_SHA256=hashlib.sha256(p.stdout+p.stderr).hexdigest())


def sdk_snapshot(path):
    root=Path(path).resolve();repos={}
    def visit(directory):
        require(git(directory,'status','--porcelain')=='','SDK worktree is dirty')
        rel=str(directory.relative_to(root));head=git(directory,'rev-parse','HEAD');links={}
        for entry in subprocess.check_output(['git','-C',str(directory),'ls-files','--stage','-z']).split(b'\0'):
            if not entry:continue
            metadata,name=entry.split(b'\t',1);mode,oid,_=metadata.decode().split()
            if mode!='160000':continue
            name=name.decode();child=(directory/name).resolve();require(child.is_relative_to(root),'SDK submodule escapes root')
            require(git(child,'rev-parse','HEAD')==oid,'SDK gitlink mismatch');links[name]=oid;visit(child)
        repos[rel]=dict(HEAD=head,gitlinks=links)
    visit(root);return dict(root=str(root),repositories=repos)


KNOWN_PROFILES = {'pico_w_lcd_160x80_4', 'pico_w_eink_250x122_0', 'pico_w_eink_250x122_2'}

def hardware_profile(root, device):
    require(device and device.replace('-', '').replace('_', '').isalnum(), 'invalid device')
    path = Path(root)/'firmware/devices'/device/'profile.txt'
    name = path.read_text().strip()
    require(name in KNOWN_PROFILES and path.read_text() == name+'\n', 'unverified hardware profile')
    header = Path(root)/'firmware/profiles'/(name+'.h')
    board_file = Path(root)/'firmware/profiles'/(name+'.board')
    board = board_file.read_text().strip()
    require(board == 'pico_w', 'unverified MCU board')
    return name, board, (path, header, board_file)

def cache(path):
    values={}
    for line in Path(path).read_text().splitlines():
        if line and not line.startswith(('#','//'))and '='in line and ':'in line.split('=',1)[0]:
            left,value=line.split('=',1);values[left.split(':',1)[0]]=value
    return values


# Existing firmware diagnostics only. Values become explicit configure options;
# no arbitrary CMake or shell arguments are accepted through the environment.
FIRMWARE_FLAGS=('PM_ACK_PATH_PROBE','PM_USB_BUTTON_SIM','PM_TX_LINK_PROBE',
    'PM_CYW43_INNER_PROBE','PM_CYW43_SPI_REAL_SIZE','PM_EVENT_STREAM','PM_STREAM_DIAGNOSTICS')


def firmware_flags():
    values={name:os.environ.get(name,'OFF')for name in FIRMWARE_FLAGS}
    require(all(v in ('ON','OFF')for v in values.values()),'firmware flags must be ON or OFF')
    return values


def snapshot(root,kind,build_dir,device=None,mode='normal',features=(),no_default=False):
    root=Path(root).resolve();build_dir=Path(build_dir).resolve()
    require(not os.environ.get('RUSTC_WRAPPER')and not os.environ.get('RUSTC_WORKSPACE_WRAPPER'),'compiler wrappers require separately modelled provenance')
    environment={name:os.environ.get(name)for name in ['RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC','RUSTUP_TOOLCHAIN','CC','CXX','CFLAGS','CXXFLAGS']}
    if kind=='firmware':
        path=build_dir/'CMakeCache.txt';values=cache(path)
        require(not values.get('CMAKE_C_COMPILER_LAUNCHER')and not values.get('CMAKE_CXX_COMPILER_LAUNCHER'),'CMake compiler launchers require separately modelled provenance')
        require(Path(values['CMAKE_HOME_DIRECTORY']).resolve()==root/'firmware','CMake source root mismatch')
        profile,profile_board,profile_files=hardware_profile(root,device)
        require(values['PM_DEVICE']==device and values['PICO_BOARD']==profile_board,'actual device/board flags mismatch')
        expected=dict(normal=('OFF','OFF','OFF'),sample=('ON','OFF','OFF'),unbound=('OFF','ON','OFF'),unbound_panel=('OFF','ON','ON'))[mode]
        require(tuple(values[n]for n in ['PM_USE_SAMPLE','PM_PREPARE_UNBOUND','PM_PREPARE_UNBOUND_PANEL'])==expected,'actual build-mode flags mismatch')
        config=root/'firmware/devices'/device/('config.sample.h'if mode=='sample'else'config.h')
        board=root/'firmware/devices'/device/('board-id.sample.txt'if mode=='sample'else'board-id.txt')
        files={str(config):sha(config), **{str(p):sha(p) for p in profile_files}}
        if mode in ['normal','sample']:files[str(board)]=sha(board)
        flags={k:v for k,v in values.items()if k in ['PM_DEVICE','PICO_BOARD','CMAKE_BUILD_TYPE','CMAKE_C_FLAGS','CMAKE_CXX_FLAGS']or(k.startswith(('PM_','PICO_'))and(v in ['ON','OFF']or v.isdecimal()))}
        commands=build_dir/'compile_commands.json';require(commands.is_file(),'actual compile commands missing')
        return dict(source_root=str(root/'firmware'),build_dir=str(build_dir),mode=mode,device=device,profile=profile,board=profile_board,
            cache_SHA256=sha(path),compile_commands_SHA256=sha(commands),flags=flags,configuration=files,
            SDK=sdk_snapshot(values['PICO_SDK_PATH']),compiler=tool(values['CMAKE_C_COMPILER']),builder=tool('cmake'),environment=environment)
    manifest=root/'service/Cargo.toml';model=tomllib.loads(manifest.read_text());known=model.get('features',{})
    require(all(f in known for f in features),'unknown Cargo feature')
    cfg=[]
    for path in [root/'.cargo/config.toml',root/'service/.cargo/config.toml',Path(os.environ.get('CARGO_HOME',str(Path.home()/'.cargo')))/'config.toml']:
        if path.is_file():
            build_config=tomllib.loads(path.read_text()).get('build',{})
            require(not any(build_config.get(k)for k in ['rustc','rustc-wrapper','rustc-workspace-wrapper']),'Cargo compiler overrides require separately modelled provenance')
            cfg.append(dict(path=str(path.resolve()),SHA256=sha(path)))
    return dict(source_root=str(root/'service'),build_dir=str(build_dir),manifest_SHA256=sha(manifest),lock_SHA256=sha(root/'service/Cargo.lock'),
        features=sorted(features),no_default=no_default,profile='release',Cargo_configuration=cfg,
        compiler=tool(os.environ.get('RUSTC','rustc')),builder=tool('cargo'),environment=environment,SDK=None)


def fingerprint(root,build_dir,features,no_default):
    model=tomllib.loads((root/'service/Cargo.toml').read_text());enabled=set(features)
    if not no_default and 'default'in model.get('features',{}):enabled.add('default')
    changed=True
    while changed:
        before=set(enabled)
        for name in list(enabled):enabled.update(x for x in model['features'].get(name,[])if x in model['features'])
        changed=enabled!=before
    pattern=model['package']['name']+'-*/bin-picomado-server.json'
    files=list((build_dir/'release/.fingerprint').glob(pattern));require(len(files)==1,'actual Cargo fingerprint ambiguous/missing')
    actual=json.loads(files[0].read_text());require(set(json.loads(actual['features']))==enabled,'actual Cargo features mismatch')
    return dict(path=str(files[0]),SHA256=sha(files[0]),features=sorted(enabled))


def run(root,args,gate):
    root=Path(root).resolve();require(not args.out or not Path(args.out).exists(),'build receipt already exists');report=gate.verify_check(args.report,'build');require(git(root,'status','--porcelain')=='','build source must be committed/clean');build_dir=Path(args.build_dir).resolve()
    require(not build_dir.exists(),'use a fresh build directory; accepted artifacts are preserved')
    build_dir.parent.mkdir(parents=True,exist_ok=True)
    features=tuple(x for x in args.features.split(',')if x)
    start=gate.input_snapshot();started=time.monotonic_ns();configure=None
    if args.kind=='firmware':
        require(args.device and args.device.replace('-','').replace('_','').isalnum(),'device required')
        requested_flags=firmware_flags()
        profile,board,_=hardware_profile(root,args.device)
        use_sample='ON'if args.mode=='sample'else'OFF';unbound='ON'if args.mode in ['unbound','unbound_panel']else'OFF'
        configure=[shutil.which('cmake'),'-S',str(root/'firmware'),'-B',str(build_dir),'-DPM_DEVICE='+args.device,
            '-DPM_USE_SAMPLE='+use_sample,'-DPM_PREPARE_UNBOUND='+unbound,'-DPM_PREPARE_UNBOUND_PANEL='+('ON'if args.mode=='unbound_panel'else'OFF'),
            '-DPICO_BOARD='+board,'-DCMAKE_BUILD_TYPE=Release','-DCMAKE_EXPORT_COMPILE_COMMANDS=ON','-DPICO_SDK_FETCH_FROM_GIT=OFF','-DPICO_SDK_SUBMODULE_AUTO_UPDATE=OFF']
        configure+=['-D'+name+'='+value for name,value in requested_flags.items()]
        with (build_dir.parent/(build_dir.name+'-configure.log')).open('xb')as log:subprocess.run(configure,stdout=log,stderr=subprocess.STDOUT,check=True)
        command=[shutil.which('cmake'),'--build',str(build_dir),'-j4','--verbose'];image=build_dir/(args.device+'.uf2')
    else:
        command=[shutil.which('cargo'),'build','--offline','--locked','--release','--manifest-path',str(root/'service/Cargo.toml'),
            '--target-dir',str(build_dir),'--bin','picomado-server','--verbose']
        if features:command+=['--features',','.join(features)]
        if args.no_default_features:command+=['--no-default-features']
        image=build_dir/'release/picomado-server'
    before=snapshot(root,args.kind,build_dir,args.device,args.mode,features,args.no_default_features)
    if args.kind=='firmware':
        require(all(before['flags'].get(k)==v for k,v in requested_flags.items()),'configured firmware flags differ from requested values')
    logpath=build_dir.parent/(build_dir.name+'-build.log')
    with logpath.open('xb')as log:subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,check=True)
    after=snapshot(root,args.kind,build_dir,args.device,args.mode,features,args.no_default_features)
    require(before==after,'build configuration/SDK/toolchain changed during build')
    require(gate.input_snapshot()['subject_digest']==start['subject_digest'],'build subject changed')
    gate.workflow_admission('build');gate.artifact(image,args.kind)
    actual_fingerprint=fingerprint(root,build_dir,features,args.no_default_features)if args.kind=='server'else None
    result=dict(schema=1,issued_by='project_gate.controlled_build.v1',change=gate.CASE,kind=args.kind,subject_digest=start['subject_digest'],source_root=str(root),
        source_commit=git(root,'rev-parse','HEAD'),build=after,command=command,configure_command=configure,exit=0,
        started_monotonic_ns=started,finished_monotonic_ns=time.monotonic_ns(),artifact_path=str(image),artifact_SHA256=sha(image),
        check_report=str(Path(args.report).resolve()),check_report_SHA256=sha(args.report),build_log=str(logpath),build_log_SHA256=sha(logpath),Cargo_fingerprint=actual_fingerprint)
    out=Path(args.out)if args.out else build_dir/'build-receipt.json';gate.write(out,result)
    return dict(built=True,artifact=str(image),build_receipt=str(out),artifact_SHA256=sha(image))


def verify(root,path,image,gate):
    root=Path(root).resolve();value=json.loads(Path(path).read_text());kind=value['kind'];b=value['build']
    require(value.get('schema')==1 and value.get('issued_by')=='project_gate.controlled_build.v1' and value.get('exit')==0 and value.get('change')==gate.CASE,'completed build receipt required')
    require(value['source_root']==str(root) and value['subject_digest']==gate.input_snapshot()['subject_digest'],'build source/subject mismatch')
    expected=snapshot(root,kind,Path(b['build_dir']),b.get('device'),b.get('mode','normal'),tuple(b.get('features',[])),b.get('no_default',False))
    require(expected==b,'actual SDK/flags/config/toolchain lineage changed')
    expected_command=[b['builder']['path'],'--build',b['build_dir'],'-j4','--verbose']if kind=='firmware'else[b['builder']['path'],'build','--offline','--locked','--release','--manifest-path',str(root/'service/Cargo.toml'),'--target-dir',b['build_dir'],'--bin','picomado-server','--verbose']
    if kind=='server':
        if b['features']:expected_command+=['--features',','.join(b['features'])]
        if b['no_default']:expected_command+=['--no-default-features']
    require(value['started_monotonic_ns']<value['finished_monotonic_ns']and value['command']==expected_command,'actual successful builder recipe required')
    require(subprocess.run(['git','merge-base','--is-ancestor',value['source_commit'],'HEAD'],cwd=root,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,'build commit is not in this lineage')
    require(sha(value['build_log'])==value['build_log_SHA256'],'build log changed')
    require(sha(image)==value['artifact_SHA256']==sha(value['artifact_path']),'artifact not from this completed build')
    require(sha(value['check_report'])==value['check_report_SHA256'],'build check receipt changed');gate.verify_check(value['check_report'])
    if kind=='server':require(fingerprint(root,Path(b['build_dir']),tuple(b['features']),b['no_default'])==value['Cargo_fingerprint'],'actual Cargo fingerprint changed')
    return value
