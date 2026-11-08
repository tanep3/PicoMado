#!/usr/bin/env python3
"""Offline RP2040 UF2 validation; reserved storage is never a load target."""
import argparse,struct
from pathlib import Path

def validate(data,flash_bytes,reserved_bytes):
    if not data or len(data)%512:raise ValueError('invalid UF2 length')
    if reserved_bytes%4096 or not 0<reserved_bytes<flash_bytes:raise ValueError('invalid storage boundary')
    count=len(data)//512;seen=set();ranges=[];limit=0x10000000+flash_bytes-reserved_bytes
    for off in range(0,len(data),512):
        m1,m2,flags,address,size,index,total,family=struct.unpack_from('<8I',data,off)
        end=struct.unpack_from('<I',data,off+508)[0]
        if (m1,m2,end)!=(0x0a324655,0x9e5d5157,0x0ab16f30):raise ValueError('UF2 magic')
        if total!=count or index>=count or index in seen:raise ValueError('UF2 block identities')
        seen.add(index)
        if flags&~0x2001 or not flags&0x2000 or family!=0xe48bff56:raise ValueError('unsupported UF2 flags/family')
        if not 0<size<=476:raise ValueError('UF2 payload length')
        if flags&1:continue
        if address%256 or size!=256 or address<0x10000000 or address+size>limit:raise ValueError('UF2 targets reserved/outside flash')
        ranges.append((address,address+size))
    if not ranges:raise ValueError('no flash payload')
    ranges.sort()
    if any(a[1]>b[0] for a,b in zip(ranges,ranges[1:])):raise ValueError('overlapping UF2 payload')
    return dict(blocks=count,flash_end=ranges[-1][1],storage_start=limit)

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('image',type=Path)
    p.add_argument('--flash-bytes',type=lambda s:int(s,0),required=True)
    p.add_argument('--reserved-bytes',type=lambda s:int(s,0),required=True)
    a=p.parse_args();v=validate(a.image.read_bytes(),a.flash_bytes,a.reserved_bytes)
    print('PASS UF2 storage boundary: blocks={blocks}, flash_end=0x{flash_end:x}, storage_start=0x{storage_start:x}'.format(**v))
