"""Expected outcomes derived from docs/capture-boundary-state-contract.md.

No collector/validator imports: this is the independent transition-table oracle.
"""
def fact(kind, sequence, boot=1, catalog=3):
    return dict(component=8 if kind in ('B','E','X') else 2,event=1,
        phase={'B':1,'E':4,'X':5,'D':4}[kind],boot=boot,sequence=sequence,us=sequence*1000,
        operation=sequence,parent=0,session=0,frame=0,dropped=0,a=catalog if kind!='D' else 0,b=0x9f if kind!='D' else 0)

B=lambda n,**kw:fact('B',n,**kw)
E=lambda n,**kw:fact('E',n,**kw)
D=lambda n,**kw:fact('D',n,**kw)
CASES=[
    dict(id='normal',rows=['T01','T02','T04','T08','T12','T13'],facts=[B(1),D(2),E(3)],end=3,complete=True),
    dict(id='old-prefix',rows=['T03'],start=3,facts=[B(1),E(2),B(3),E(4)],end=4,complete=True),
    dict(id='later-suffix',rows=['T10'],facts=[B(1),E(2),B(3),E(4),D(5,boot=2)],end=2,complete=True),
    dict(id='duplicate-begin',rows=['T06'],facts=[B(1),B(1),E(2)],end=2,complete=False,error='duplicate_begin'),
    dict(id='distinct-begin',rows=['T06'],facts=[B(1),B(2),E(3)],end=3,complete=False,error='unexpected_begin'),
    dict(id='lower-begin',rows=['T06'],start=3,facts=[B(3),D(4),B(2),E(5)],end=5,complete=False,error='unexpected_begin'),
    dict(id='catalog-changed-begin',rows=['T06'],facts=[B(1),B(2,catalog=99),E(3)],end=3,complete=False,error='unexpected_begin'),
    dict(id='wrong-first-begin',rows=['T05'],facts=[B(2),E(3)],end=3,complete=False,error='unexpected_begin'),
    dict(id='late-first-begin',rows=['T05'],facts=[D(2),B(1),E(3)],end=3,complete=False,error='begin_reordered'),
    dict(id='anchor-sequence-reused',rows=['T12'],facts=[D(1),B(1),E(2)],end=2,complete=False,error='sequence_gap_or_reorder'),
    dict(id='invalid-then-matching',rows=['T05'],facts=[B(2),B(1),E(3)],end=3,complete=False,error='unexpected_begin'),
    dict(id='foreign-begin',rows=['T07'],facts=[B(1),B(2,boot=2),E(2)],end=2,complete=False,error='boot_changed'),
    dict(id='foreign-data',rows=['T07'],facts=[B(1),D(2,boot=2),E(2)],end=2,complete=False,error='boot_changed'),
    dict(id='foreign-unproven-prefix',rows=['T07'],facts=[D(9,boot=2),B(1),E(2)],end=2,complete=False,error='boot_changed'),
    dict(id='end-before-begin',rows=['T09'],facts=[E(2),B(1)],end=2,complete=False,error='unexpected_end'),
    dict(id='duplicate-end',rows=['T11'],facts=[B(1),E(2),E(2)],end=2,complete=False,error='duplicate_end'),
    dict(id='late-duplicate-begin',rows=['T11'],facts=[B(1),E(2),B(1)],end=2,complete=False,error='duplicate_begin'),
    dict(id='off-past-first-end',rows=['T14'],facts=[B(1),E(2),E(3)],end=3,complete=False,error='end_watermark_gap'),
    dict(id='missing-begin',rows=['T09'],facts=[D(2),E(3)],end=3,complete=False,error='unexpected_end'),
    dict(id='missing-end',rows=['T14'],facts=[B(1),D(2)],end=2,complete=False),
    dict(id='missing-off',rows=['T14'],facts=[B(1),D(2)],end=None,complete=False,error='missing_boundary'),
    dict(id='partial-final-line',rows=['T14'],facts=[B(1),E(2)],end=2,partial=True,complete=False,error='partial_line'),
    dict(id='foreign-off',rows=['T14'],facts=[B(1),E(2)],end=2,end_boot=2,complete=False,error='end_boot_changed'),
    dict(id='off-still-enabled',rows=['T14'],facts=[B(1),E(2)],end=2,end_on=True,complete=False,error='invalid_end'),
    dict(id='invalid-boundary-phase',rows=['T06'],facts=[B(1),fact('X',2),E(3)],end=3,complete=False,error='invalid_boundary_phase'),
]
