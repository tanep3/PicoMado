#!/usr/bin/env python3
"""Offline format and storage-boundary negatives against the product validator."""
import importlib.util,struct,unittest
from pathlib import Path
p=Path(__file__).resolve().parents[1]/'tools/check_uf2_storage.py'
s=importlib.util.spec_from_file_location('check_uf2_storage',p);m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
SIZE=2097152;RESERVED=16384;START=0x10000000+SIZE-RESERVED

def block(address,index=0,total=1):
 b=bytearray(512);struct.pack_into('<8I',b,0,0x0a324655,0x9e5d5157,0x2000,address,256,index,total,0xe48bff56);struct.pack_into('<I',b,508,0x0ab16f30);return b
class Bounds(unittest.TestCase):
 def test_last_allowed_page(self):self.assertEqual(m.validate(block(START-256),SIZE,RESERVED)['flash_end'],START)
 def test_cache_bt_and_settings_rejected(self):
  for address in range(START,0x10000000+SIZE,256):
   with self.subTest(address=address),self.assertRaises(ValueError):m.validate(block(address),SIZE,RESERVED)
 def test_invalid_headers(self):
  for offset,value in ((0,0),(4,0),(8,0),(12,START-255),(16,0),(20,1),(24,2),(28,0),(508,0)):
   b=block(START-256);struct.pack_into('<I',b,offset,value)
   with self.subTest(offset=offset),self.assertRaises(ValueError):m.validate(b,SIZE,RESERVED)
 def test_overlap_duplicate_and_truncated(self):
  for b in (block(START-256,0,2)+block(START-256,1,2),block(START-512,0,2)+block(START-256,0,2),block(START-256)[:-1],b''):
   with self.assertRaises(ValueError):m.validate(b,SIZE,RESERVED)
 def test_entire_image(self):
  self.assertEqual(m.validate(block(0x10000000,0,2)+block(START-256,1,2),SIZE,RESERVED)['blocks'],2)
if __name__=='__main__':unittest.main(verbosity=2)
