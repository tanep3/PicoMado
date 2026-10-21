#!/usr/bin/env python3
"""Conservative offline cause/frame analysis. Never subtract host and device clocks."""
import argparse
import json
import hashlib
import struct
from pathlib import Path
from event_stream import Window, decode, parse_status, BODY, FIELDS, MAX_U64

MAX_BYTES = 64*1024*1024


def bounded_lines(path):
    if Path(path).stat().st_size > MAX_BYTES:
        raise ValueError('evidence exceeds 64MiB')
    return Path(path).read_bytes().splitlines(keepends=True)


def interval_source(facts, start, end, reasons):
    """Validate the interval before returning only its source/sequence facts."""
    try:
        markers=[(i,f) for i,f in enumerate(facts) if f['component']==8 and f['event']==1 and f['a'] in (2,3) and (f['b']&0x9f)==0x9f]
        begins=[(i,f) for i,f in markers if f['phase']==1 and f['sequence']==start['sequence'] and f['boot']==start['boot']]
        ends=[(i,f) for i,f in markers if f['phase']==4 and f['sequence']==end['sequence'] and f['boot']==end['boot']]
        if len(begins)!=1 or len(ends)!=1:
            reasons.append('catalog_or_coverage')
        elif begins[0][0]>ends[0][0] or any(f['boot']!=start['boot'] for f in facts[begins[0][0]:ends[0][0]+1]):
            reasons.append('boot_or_boundary_order')
        selected=[f for f in facts if f['boot']==start['boot'] and start['sequence']<f['sequence']<=end['sequence']]
        window=Window(1);window.begin(start)
        # Replay the real begin and every in-interval boundary through the same
        # transition table as the collector. Window owns prefix/suffix exclusion.
        for fact in facts:window.accept(fact)
        result=window.finish(end);reasons.extend(result['reasons'])
        return selected,result['causal_complete'] and not reasons,reasons,ends[0][1]['us'] if len(ends)==1 else 0
    except (KeyError,TypeError,ValueError):
        return [],False,reasons+['invalid_boundary_or_event'],0


def collector_source(path, lines):
    facts=[];reasons=[]
    try:
        summary=json.loads(Path(str(path)+'.summary.json').read_bytes())
        segments=summary['segments'];storage=summary['storage']
        if len(segments)!=1:return [],False,['ambiguous_collector_segments'],0
        segment=segments[0];epoch=segment['link_epoch'];start=segment['start'];end=segment['end']
        if not start or not end:return [],False,['missing_collector_boundary'],0
        if not summary.get('transport_complete') or not summary.get('storage_durable') or summary.get('interrupted') or not segment.get('transport_complete'):
            reasons.append('collector_incomplete')
        rows=[json.loads(line) for line in lines]
        if storage.get('attempted')!=len(rows) or storage.get('durable_prefix')!=len(rows) or storage.get('last_durable_host_sequence')!=len(rows) or storage.get('queue_lost') or storage.get('write_lost') or storage.get('error') or storage.get('worker_alive') or storage.get('bytes')!=Path(path).stat().st_size:
            reasons.append('collector_storage_unconfirmed')
        previous_ns=0;receipts={};written={};queued={}
        for sequence,row in enumerate(rows,1):
            if row.get('host_sequence')!=sequence or not isinstance(row.get('host_mono_ns'),int) or row['host_mono_ns']<previous_ns:
                reasons.append('collector_host_sequence_or_time')
            previous_ns=row.get('host_mono_ns',0)
            if row.get('link_epoch')!=epoch:continue
            if row.get('kind')=='command_queued':queued[row.get('command_id')]=(sequence,row.get('command',''))
            if row.get('kind')=='command_written':written[row.get('command_id')]=sequence
            if row.get('kind')=='line':
                status=parse_status((row.get('line','')+'\n').encode('ascii'))
                if status is not None:receipts.setdefault(status['cid'],[]).append((sequence,status))
            if row.get('kind') in ('decode_error','partial_line'):reasons.append('collector_source_error')
            if row.get('kind')=='event':
                fact=row['event']
                if set(fact)!=set(FIELDS) or any(type(fact[k])is not int or fact[k]<0 or fact[k]>MAX_U64 for k in FIELDS):
                    raise ValueError('invalid numeric event')
                BODY.pack(*(fact[k] for k in FIELDS))
                facts.append(fact)
        for command,boundary in [('ON',start),('OFF',end)]:
            queued_sequence,wire=queued.get(boundary['cid'],(0,''))
            expected=['EV1',command,format(boundary['cid'],'x')]
            if wire.split()!=expected or not any(status==boundary and 0<queued_sequence<written.get(boundary['cid'],0)<sequence for sequence,status in receipts.get(boundary['cid'],[])):
                reasons.append('collector_boundary_provenance')
        return interval_source(facts,start,end,reasons)
    except (OSError,ValueError,KeyError,TypeError,UnicodeError,OverflowError,struct.error):
        return [],False,['invalid_or_missing_collector_artifact'],0


def device_source(path):
    lines=bounded_lines(path)
    if lines and lines[0].lstrip().startswith(b'{'):
        return collector_source(path,lines)
    facts=[];statuses=[];reasons=[]
    for line in lines:
        if not line.endswith(b'\n'):reasons.append('partial_line')
        try:
            status=parse_status(line);fact=decode(line)
            if status:statuses.append(status)
            if fact:facts.append(fact)
        except ValueError:reasons.append('decode_error')
    starts=[s for s in statuses if s['on']];ends=[s for s in statuses if not s['on']]
    if len(starts)!=1 or len(ends)!=1:return [],False,['ambiguous_or_missing_boundary'],0
    return interval_source(facts,starts[0],ends[0],reasons)


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
    if any(r.get('catalog') not in (2,3) or
           (r.get('coverage',0)&(0xef if r.get('catalog')==3 else 0xeb))!=(0xef if r.get('catalog')==3 else 0xeb)
           for r in [begin,end]):
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
                if any(f['component']==4 and f['phase']==1 and f['frame']==frame for f in df):
                    result.update(status='unknown',reasons=['frame_cause_unresolved'])
                    break
                expected='device_frame_start'
                continue
            complete = [f for f in df if f['component']==4 and f['phase']==4 and f['frame']==frame]
            draws = [f for f in df if f['component']==5 and f['phase']==4 and f['frame']==frame]
            failed = [f for f in df if f['component'] in [4,5] and f['phase'] in [5,6] and f['frame']==frame]
            cancelled=[f for f in df if f['component'] in [4,5] and f['phase']==7 and f['frame']==frame]
            if failed:
                result.update(status='explicit_failure',frame=frame,phase=failed[-1]['phase'])
                break
            if cancelled:
                result.update(status='cancelled',frame=frame,reasons=['explicit_cancellation'])
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
    return dict(transport_complete=dc,server_complete=sc,device_reasons=dr,server_reasons=sr,results=results,
                boundary_limits=dict(pre_first_observation='unknown',
                    raw_read='userspace read completion, not kernel arrival',
                    raw_to_ws='unknown without exact transport consumption provenance',
                    cross_source_duration='unknown without an explicit clock mapping'))


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
