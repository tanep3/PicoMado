import unittest,os,sys,tempfile,json,copy
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
import cyw43_inner_collect as c
import usb_button_trial as wire
from unittest.mock import patch
class Export:
 def __init__(self):self.lines=Path(os.environ['PM_CYW_EXPORT_FIXTURE']).read_text().splitlines();self.requests=[];self.status_calls=0;self.missing=None;self.corrupt=False;self.overflow=False;self.stale=False;self.schema=False;self.change=False
 def request(self,cmd,end):
  self.requests.append(cmd)
  if cmd=='CY1 CAPS':line=next(x for x in self.lines if x.startswith('cy1 caps '));return[line.replace('v=1','v=2')if self.schema else line]
  if cmd=='CY1 STATUS':
   self.status_calls+=1;line=next(x for x in self.lines if x.startswith('cy1 status '))
   if self.overflow:line=line.replace('over=0','over=1')
   if self.stale:line=line.replace('r=a','r=b')
   if self.change and self.status_calls>1:line=line.replace('n=2','n=1')
   return[line]
  if cmd=='CY1 QUALITY':return[next(x for x in self.lines if x.startswith('cy1 quality '))]
  i,p=cmd.split()[4:]
  if self.missing==(i,p):raise EOFError('collection interruption')
  line=next(x for x in self.lines if x.startswith(f'cy1 row i={i} p={p} '))
  if self.corrupt and i=='0'and p=='3':
   v=c.one(type('U',(),{'request':lambda *_:[line]})(),'','cy1 row ');line=line.replace('c0='+v['c0'],'c0=0')
  return[line]
class Tests(unittest.TestCase):
 def test_actual_serial_parser_collects_CY1_caps_and_all_parts(self):
  backend=Export();pending=[];commands=[]
  def write(fd,data):
   self.assertEqual(fd,123);command=data.decode().strip();commands.append(command)
   response=('stats unrelated=1\n'+'\n'.join(backend.request(command,''))+'\n').encode();cut=max(1,len(response)//2);pending.extend([response[:cut],response[cut:]]);return len(data)
  def select(readers,writers,errors,timeout):return ([123]if pending else[],writers,[])
  transport=wire.Serial('/dev/fake');transport.fd=123
  with patch.object(wire.os,'write',side_effect=write),patch.object(wire.os,'read',side_effect=lambda *_:pending.pop(0)),patch.object(wire.select,'select',side_effect=select):
   result=c.collect(transport,'1','a')
  transport.fd=None;self.assertEqual(len(result['rows']),2);self.assertTrue(result['no_diagnostic_loss']);self.assertEqual(commands[0],'CY1 CAPS');self.assertIn('CY1 READ 1 a 1 5',commands)
 def test_actual_serial_parser_reports_CY1_error(self):
  transport=wire.Serial('/dev/fake');transport.fd=123
  with patch.object(wire.os,'write',side_effect=lambda fd,data:len(data)),patch.object(wire.os,'read',return_value=b'cy1 error=identity_or_state\n'),patch.object(wire.select,'select',return_value=([123],[123],[])):
   with self.assertRaisesRegex(ValueError,'cy1 error=identity_or_state'):transport.request('CY1 READ 1 a 0 0','cy1 row ')
  transport.fd=None
 def test_real_C_export_digest_roundtrip(self):
  v=c.collect(Export(),'1','a');self.assertEqual(len(v['rows']),2);self.assertEqual(v['classifications'],['no_credit_stall','bus_error_or_no_bus_write']);self.assertFalse(v['RF_departure_proven'])
 def test_parts_persist_before_complete(self):
  with tempfile.TemporaryDirectory()as d:
   v=c.collect(Export(),'1','a',Path(d));self.assertEqual(len(list((Path(d)/'cyw43-parts').glob('*.json'))),13);self.assertEqual(json.loads((Path(d)/'cyw43.json').read_text())['status'],v['status'])
 def test_interruption_retains_prior_parts(self):
  usb=Export();usb.missing=('1','2')
  with tempfile.TemporaryDirectory()as d:
   with self.assertRaises(EOFError):c.collect(usb,'1','a',Path(d))
   self.assertTrue((Path(d)/'cyw43-parts/0-0.json').exists());self.assertTrue((Path(d)/'cyw43-parts/1-1.json').exists());self.assertFalse((Path(d)/'cyw43.json').exists())
 def test_crc_part_corruption_rejected(self):
  usb=Export();usb.corrupt=True
  with self.assertRaisesRegex(ValueError,'digest'):c.collect(usb,'1','a')
 def test_changed_frozen_rejected(self):
  usb=Export();usb.change=True
  with self.assertRaisesRegex(ValueError,'changed'):c.collect(usb,'1','a')
 def test_wrong_schema_rejected_before_status(self):
  usb=Export();usb.schema=True
  with self.assertRaisesRegex(ValueError,'schema'):c.collect(usb,'1','a')
  self.assertEqual(usb.requests,['CY1 CAPS'])
 def test_stale_binding_rejected_before_pages(self):
  usb=Export();usb.stale=True
  with self.assertRaisesRegex(ValueError,'binding'):c.collect(usb,'1','a')
  self.assertEqual(usb.status_calls,1);self.assertFalse(any('READ'in x for x in usb.requests))
 def test_overflow_never_clean(self):
  usb=Export();usb.overflow=True;v=c.collect(usb,'1','a');self.assertFalse(v['no_diagnostic_loss']);self.assertEqual(v['classifications'],['incomplete']*2)
 def test_positive_change_with_RX_and_bus_success(self):
  v=c.collect(Export(),'1','a');row=copy.deepcopy(v['rows'][1]);row[5]['brc']=0;self.assertEqual(c.classify(row),'stall_RX_observed_region_changed')
 def test_no_stall_change_keeps_distinct_class(self):
  row=copy.deepcopy(c.collect(Export(),'1','a')['rows'][1]);row[0]['flags']&=~2;row[5]['brc']=0;self.assertEqual(c.classify(row),'no_credit_stall_region_changed')
 def test_signed_fields_are_explicit_little_endian(self):
  rows=c.collect(Export(),'1','a')['rows'];r=copy.deepcopy(rows);r[1][5]['brc']&=0xffffffff;r[1][4]['rxrc']&=0xffff;self.assertEqual(c.digest(rows),c.digest(r))
if __name__=='__main__':unittest.main()
