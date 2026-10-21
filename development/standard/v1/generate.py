#!/usr/bin/env python3
"""Small data/state projection generator. No project rules or IO adapters here."""
import argparse
import hashlib
import json
from pathlib import Path


def render(spec):
    tag = hashlib.sha256(json.dumps(spec, sort_keys=True).encode()).hexdigest()
    c = ['/* Generated; edit contracts/observation-domain.json. '+tag+' */', '#pragma once', '#include <stdint.h>', '#include <stdbool.h>']
    rs = ['// Generated; edit contracts/observation-domain.json. '+tag, '#![allow(dead_code, non_camel_case_types)]']
    py = ['# Generated; edit contracts/observation-domain.json. '+tag, 'from dataclasses import dataclass', 'from enum import Enum']
    for name, unit in spec['values'].items():
        bits = int(unit[1:]); ctype = 'uint'+str(bits)+'_t'
        c += [f'typedef struct {{ {ctype} value; }} obs_{name};',
              f'static inline obs_{name} obs_{name}_from_wire({ctype} v){{return (obs_{name}){{v}};}}',
              f'static inline {ctype} obs_{name}_wire(obs_{name} v){{return v.value;}}']
        rs += ['#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]', f'pub struct {name}({unit});',
               f'impl {name} {{ pub fn from_wire(v:{unit})->Self{{Self(v)}} pub fn wire(self)->{unit}{{self.0}} }}']
        py += ['@dataclass(frozen=True, slots=True)', f'class {name}:', '    value: int',
               '    def __post_init__(self):', f'        if type(self.value) is not int or not 0 <= self.value < 2**{bits}: raise ValueError("{name}")']
    for owner, model in spec['states'].items():
        states = model['states']; events = sorted({e for _, e, _ in model['transitions']})
        c += [f'typedef enum {{{",".join("obs_"+owner+"_"+s for s in states)}}} obs_{owner}_state;',
              f'typedef enum {{{",".join("obs_"+owner+"_"+e for e in events)}}} obs_{owner}_event;',
              f'static inline bool obs_{owner}_next(obs_{owner}_state s,obs_{owner}_event e,obs_{owner}_state *out){{']
        c += [f' if(s==obs_{owner}_{a} && e==obs_{owner}_{e}){{*out=obs_{owner}_{b};return true;}}' for a,e,b in model['transitions']]
        c += [' return false;', '}']
        prefix = owner.title()
        rs += ['#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]', '#[serde(rename_all="snake_case")]', f'pub enum {prefix}State {{{",".join(states)}}}',
               '#[derive(Clone, Copy, Debug)]', f'pub enum {prefix}Event {{{",".join(events)}}}',
               f'pub struct {prefix}Window {{ state:{prefix}State }}',
               f'impl {prefix}Window {{ pub fn new()->Self{{Self{{state:{prefix}State::{model["initial"]}}}}} pub fn state(&self)->{prefix}State{{self.state}}',
               f'pub fn apply(&mut self,event:{prefix}Event)->Result<(), &\'static str>{{self.state=match(self.state,event){{']
        rs += [f'({prefix}State::{a},{prefix}Event::{e})=>{prefix}State::{b},' for a,e,b in model['transitions']]
        rs += ['_=>return Err("invalid transition"),};Ok(())}}']
        py += [f'class {prefix}State(Enum):']+[f'    {s} = "{s}"' for s in states]
        py += [f'{owner.upper()}_TRANSITIONS = '+repr({(a,e):b for a,e,b in model['transitions']})]
    return {spec['outputs']['c']:'\n'.join(c)+'\n', spec['outputs']['rust']:'\n'.join(rs)+'\n', spec['outputs']['python']:'\n'.join(py)+'\n'}


def main():
    p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--check',action='store_true');a=p.parse_args()
    spec=json.loads((a.root/'contracts/observation-domain.json').read_text());bad=[]
    for path,body in render(spec).items():
        file=a.root/path
        if a.check:
            if not file.exists() or file.read_text()!=body:bad.append(path)
        else:file.write_text(body)
    if bad:raise SystemExit('generated drift: '+', '.join(bad))


if __name__=='__main__':main()
