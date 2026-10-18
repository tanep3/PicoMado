import importlib.util,sys
from pathlib import Path
module=Path(__file__).resolve().parents[1]/'tools/parse_rx_dump.py'
spec=importlib.util.spec_from_file_location('rx_parser',module);parser=importlib.util.module_from_spec(spec);spec.loader.exec_module(parser)
text=Path(sys.argv[1]).read_text();result=parser.parse(text)
assert len(result)==1 and result[0]['complete'],result
assert isinstance(result[0]['records'][0]['id']['frame'],int)
assert result[0]['records'][0]['id']['frame']==900
bad=[text.replace('rx end epoch=1\n',''),
     '\n'.join(l for l in text.splitlines() if not ('slot=1 part=gap0 ' in l)),
     text+next(l for l in text.splitlines() if 'slot=0 part=time ' in l)+'\n',
     text.replace('part=gap0 rx=0 credit=0 anchor=0 tail=1','part=gap0 rx=0'),
     text.replace('schema=2','schema=99'),
     text.replace('metric=rx_cb','metric=unknown')]
for i,s in enumerate(bad):
    assert s!=text,(i,'corruption fixture did not match')
    assert not parser.parse(s)[0]['complete'],(i,parser.parse(s))
for limit in [1,10,len(text)//2,len(text)-5]:assert not all(r['complete'] for r in parser.parse(text[:limit])) if parser.parse(text[:limit]) else True
print('rx parser PASS: complete snapshot accepted; truncation, missing/duplicate parts, missing fields, unknown schema/metrics rejected')
