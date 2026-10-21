"""Ordinary callers cannot substitute a raw physical index for an operation."""
from pathlib import Path
import subprocess
root=Path(__file__).resolve().parents[1]
base=['cc','-std=c11','-Wall','-Wextra','-Werror','-fsyntax-only','-Isrc','-x','c','-']
valid='#include "input_rules.h"\nint main(void){pm_operation op={0};if(!pm_operation_from_index(1,&op))return 1;return pm_input_decide((pm_input_view){ACTIVE,0,PM_OPERATION_MASK,0},op).result!=PM_INPUT_READY;}\n'
r=subprocess.run(base,input=valid,text=True,capture_output=True,cwd=root);assert r.returncode==0,r.stderr
for bad in ['pm_input_decide((pm_input_view){0},1);','pm_operation op=1;(void)op;','pm_input_handler handler=(pm_input_result (*)(unsigned))0;(void)handler;']:
    r=subprocess.run(base,input='#include "input_rules.h"\nint main(void){'+bad+'return 0;}\n',text=True,capture_output=True,cwd=root)
    assert r.returncode!=0,'raw integer/type substitution was accepted'
print('operation value type: valid factory API and three compiler refusals PASS')
