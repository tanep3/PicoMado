#!/usr/bin/env python3
"""Conservative offline cause/frame analysis. Never subtract host and device clocks."""
import argparse
import json
import hashlib
from pathlib import Path
from event_stream import Window, decode, parse_status

MAX_BYTES = 64*1024*1024


def bounded_lines(path):
    if Path(path).stat().st_size > MAX_BYTES:
        raise ValueError('evidence exceeds 64MiB')
    return Path(path).read_bytes().splitlines(keepends=True)


def device_source(path):
    facts, statuses, reasons = [], [], []
    for line in bounded_lines(path):
        if not line.endswith(b'\n'):
            reasons.append('partial_line')
        try:
            status = parse_status(line)
            fact = decode(line)
            if status:
                statuses.append(status)
            if fact:
                facts.append(fact)
        except ValueError:
            reasons.append('decode_error')
    starts = [s for s in statuses if s['on']]
    ends = [s for s in statuses if not s['on']]
    if len(starts) != 1 or len(ends) != 1:
        return facts, False, ['ambiguous_or_missing_boundary'], 0
    start, end = starts[0], ends[0]
    window = Window(1)
    window.begin(start)
    for fact in facts:
        if fact['sequence'] > start['sequence']:
            window.accept(fact)
    result = window.finish(end)
    markers = [f for f in facts if f['component']==8 and f['event']==1 and f['a']==2 and (f['b'] & 0x9f)==0x9f]
    begin_marker = [f for f in markers if f['phase']==1 and f['sequence']==start['sequence'] and f['boot']==start['boot']]
    end_marker = [f for f in markers if f['phase']==4 and f['sequence']==end['sequence'] and f['boot']==end['boot']]
    if len(begin_marker)!=1 or len(end_marker)!=1:
        reasons.append('catalog_or_coverage')
    reasons.extend(result['reasons'])
    return facts, result['transport_complete'] and not reasons, reasons, end_marker[0]['us'] if len(end_marker)==1 else 0


def server_source(path):
    rows = [json.loads(line) for line in bounded_lines(path)]
    facts = [r['event'] for r in rows if r.get('kind')=='event']
    if not rows or rows[0].get('kind')!='begin' or rows[-1].get('kind')!='end':
        return facts, False, ['missing_server_boundary']
    begin, end = rows[0], rows[-1]
    reasons = []
    try:
        manifest=json.loads(Path(str(path)+'.summary.json').read_bytes())
        digest=hashlib.sha256(Path(path).read_bytes()).hexdigest()
        if not manifest.get('storage_durable') or not manifest.get('complete') or manifest.get('source_sha256')!=digest:
            reasons.append('server_storage_unconfirmed')
    except (OSError,ValueError):
        reasons.append('server_storage_unconfirmed')
    if any(r.get('catalog')!=2 or (r.get('coverage',0)&0xeb)!=0xeb for r in [begin,end]):
        reasons.append('server_catalog_or_coverage')
    a,b = begin['status'], end['status']
    if not a['boot'] or a['boot']!=b['boot'] or b.get('saturated') or a['dropped']!=b['dropped'] or not end.get('complete'):
        reasons.append('server_source_incomplete')
    last, us = a['sequence'], begin['us']
    for fact in facts:
        if fact['boot']!=a['boot'] or fact['sequence']!=last+1 or fact['us']<us:
            reasons.append('server_sequence_or_time')
        last, us = fact['sequence'], fact['us']
    if last!=b['sequence']:
        reasons.append('server_end_gap')
    return facts, not reasons, reasons


def analyze(device, server, *, slow_us=500000, deadline_us=3000000, require_frame=False):
    df, dc, dr, end_us = device
    sf, sc, sr = server
    roots = [f for f in df if f['component']==1 and f['phase']==1]
    results = []
    for root in roots:
        cause = dict(version=1, boot=root['boot'], operation=root['operation'])
        result = dict(cause=cause, status='unknown', reasons=dr+sr)
        results.append(result)
        if not dc or not sc:
            result['status']='incomplete'
            continue
        host = [f for f in sf if f.get('cause')==cause]
        if not host:
            result['reasons']=['cause_not_resolved']
            continue
        sent = any(f['component']==3 and f['parent']==root['operation'] and f['phase']==3 for f in df)
        api = any(f['component']=='Api' and f['phase'] in ['Received','Dispatched'] for f in host)
        committed = any(f['component']=='State' and f['phase']=='Committed' for f in host)
        frames = [f['frame'] for f in host if f['component']=='Frame' and f['phase']=='Pending' and f.get('frame')]
        if not frames and not require_frame:
            result['reasons']=['no_frame_requirement_or_assignment']
            continue
        expected = 'device_notification' if not sent else 'server_api' if not api else 'server_commit' if not committed else 'server_frame_assignment'
        for frame in dict.fromkeys(frames):
            starts = [f for f in df if f['component']==4 and f['phase']==1 and f['frame']==frame and f['parent']==root['operation']]
            if not starts:
                expected='device_frame_start'
                continue
            complete = [f for f in df if f['component']==4 and f['phase']==4 and f['frame']==frame]
            draws = [f for f in df if f['component']==5 and f['phase']==4 and f['frame']==frame]
            failed = [f for f in df if f['component'] in [4,5] and f['phase'] in [5,6,7] and f['frame']==frame]
            if failed:
                result.update(status='explicit_failure', frame=frame, phase=failed[-1]['phase'])
                break
            if sent and api and committed and complete and draws:
                elapsed=draws[0]['us']-root['us']
                if elapsed<0:
                    result.update(status='incomplete', reasons=['device_time_reversed'])
                else:
                    result.update(status='slow' if elapsed>=slow_us else 'rendered',frame=frame,elapsed_device_us=elapsed)
                break
            expected='device_frame_complete' if not complete else 'display_complete'
        else:
            later = any(f['boot']==root['boot'] and f['us']>=root['us'] and f['sequence']>root['sequence'] for f in roots)
            if later:
                result['reasons']=['later_input_may_supersede']
            elif end_us-root['us']<deadline_us:
                result['reasons']=['capture_ended_before_deadline']
            else:
                result.update(status='missing_after_deadline', boundary=expected)
    return dict(transport_complete=dc, server_complete=sc, results=results)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device',required=True)
    parser.add_argument('--server',required=True)
    parser.add_argument('--slow-us',type=int,default=500000)
    parser.add_argument('--deadline-us',type=int,default=3000000)
    parser.add_argument('--require-frame',action='store_true')
    args=parser.parse_args()
    if args.slow_us<0 or args.deadline_us<=0:
        parser.error('non-negative slow threshold and positive deadline required')
    print(json.dumps(analyze(device_source(args.device),server_source(args.server),slow_us=args.slow_us,deadline_us=args.deadline_us,require_frame=args.require_frame),indent=2))


if __name__=='__main__':
    main()
