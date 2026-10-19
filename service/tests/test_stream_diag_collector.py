import importlib.util,json,subprocess,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('collector',Path(__file__).resolve().parents[1]/'tools/collect_stream_diag.py')
collector=importlib.util.module_from_spec(spec);spec.loader.exec_module(collector)

class CollectorFailures(unittest.TestCase):
 def exercise(self,fail):
  clock=[0.0]
  def mono():clock[0]+=.05;return clock[0]
  def sleep(n):clock[0]+=n
  def run(args,**kwargs):
   if args[0]=='ss':
    if fail in ['ss','both']:raise subprocess.TimeoutExpired(args,5,output=b'partial socket',stderr=b'slow ss')
    return subprocess.CompletedProcess(args,0,stdout='socket',stderr='')
   kwargs['stdout'].write('{"partial_journal":true}\n');kwargs['stdout'].flush()
   if fail in ['journal','both']:raise subprocess.TimeoutExpired(args,30,stderr=b'slow journal')
   return subprocess.CompletedProcess(args,0,stderr='')
  with tempfile.TemporaryDirectory() as temp:
   out=Path(temp)/'evidence'
   with patch.object(collector,'process',return_value={'starttime':1}),patch.object(collector.time,'monotonic',side_effect=mono),patch.object(collector.time,'sleep',side_effect=sleep),patch.object(collector.subprocess,'run',side_effect=run):
    result=collector.collect(123,3,out)
   self.assertTrue((out/'host.jsonl').exists());self.assertTrue((out/'journal.jsonl').exists());self.assertTrue((out/'result.json').exists())
   saved=json.loads((out/'result.json').read_text());self.assertEqual(saved,result)
   self.assertGreaterEqual(saved['samples'],2);self.assertTrue(saved['sampling_finished'])
   self.assertEqual(saved['complete'],fail=='none')
   if fail in ['ss','both']:
    row=json.loads((out/'host.jsonl').read_text().splitlines()[0]);self.assertEqual(row['ss'],'partial socket');self.assertEqual(row['ss_error']['kind'],'timeout')
   if fail in ['journal','both']:self.assertEqual((out/'journal.jsonl').read_text(),'{"partial_journal":true}\n')
   return saved
 def test_ss_timeout_preserves_result_and_continues(self):self.exercise('ss')
 def test_journal_timeout_preserves_partial_output(self):self.exercise('journal')
 def test_both_timeouts_report_both_commands(self):self.assertEqual({x['command'][0] for x in self.exercise('both')['errors']},{'ss','journalctl'})
 def test_success_is_complete(self):self.exercise('none')

if __name__=='__main__':unittest.main()
