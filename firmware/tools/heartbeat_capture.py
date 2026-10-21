"""Natural-heartbeat probe primitives and strict offline validator.

No CLI opens USB or sends commands. A reviewed single owner supplies transports;
prepare/observe/collect/release are separate, bounded, retained-evidence phases.
"""
import argparse
import hashlib
import json
import time
import zlib
from pathlib import Path
import ack_probe_v2 as ap
import cyw43_inner_collect as cy
import usb_button_trial as txlib
from observation_serialization import report_value

CAPS = {'v': '3', 'hb': '1', 'event': '19'}
WINDOW = 20.0
MAX_WAIT = 16.0


def require(ok, why):
    if not ok:
        raise ValueError(why)


def one(usb, command, prefix):
    return txlib.one(usb, command, prefix)


def prepare(usb, host, device, run, out):
    """The existing exclusive owner stays open; DTR is high during preparation.

    A refusal retains the partial arm for explicit freeze/save/release. Mutating
    commands are never retried, including an uncertain reply timeout.
    """
    require(getattr(usb, 'fd', None) is not None, 'single USB owner required')
    require(one(usb, 'AP2 CAPS', 'ap2 caps ') == CAPS, 'heartbeat capability mismatch')
    cy.capabilities(usb)
    ps = one(usb, 'AP2 STATUS', 'ap2 status ')
    ev1 = one(usb, 'EV1 STATUS', 'ev1 status ')
    require(ev1['b'] == ps['boot'] and ev1['on'] == ev1['sat'] == '0', 'EV1 must already be OFF')
    hs = host({'op': 'status'})
    require(ps['state'] == '0' and hs['phase'] == 'idle', 'retained run; no rearm')
    require(hs.get('heartbeat_trigger') == 1 and hs['target'] == device
            and hs['connection'] and hs['session'], 'host heartbeat binding unsupported')
    ident = dict(pico_boot=ps['boot'], host_boot=hs['boot'], run=run,
                 device=device, host_connection=hs['connection'], session=hs['session'])
    ap.persist(Path(out) / 'identity.json', ident)
    armed = one(usb, f'AP2 ARM_HB {ps["boot"]} {run}', 'ap2 status ')
    ap.require_status(armed, ps['boot'], run, 4)
    origin = one(usb, 'AP2 TRIGGER', 'ap2 trigger ')
    require(origin['b'] == ps['boot'] and origin['r'] == run and origin['mode'] == '1'
            and origin['m'] == origin['flags'] == '0', 'Pico heartbeat arm provenance')
    baseline = int(origin['o'], 16)
    require(baseline < 0xffffffff, 'ordinal overflow')
    # Save endpoint-scoped readings before the one-shot arm. The ARM snapshot
    # alone cannot show a send during PREPARED, and cleanup status is too late
    # to identify the refusal-time host watermark.
    pre_host_start = time.monotonic_ns()
    pre_host = host({'op': 'status'})
    pre_host_end = time.monotonic_ns()
    pre_pico_start = time.monotonic_ns()
    pre_pico = one(usb, 'AP2 LIVE', 'ap2 live ')
    pre_pico_end = time.monotonic_ns()
    bracket = dict(clock='CLOCK_MONOTONIC', origin=origin,
                   host_before=dict(value=pre_host, request_before_ns=pre_host_start,
                                    response_after_ns=pre_host_end),
                   pico_before=dict(value=pre_pico, request_before_ns=pre_pico_start,
                                    response_after_ns=pre_pico_end))
    ap.persist(Path(out) / 'baseline-before-arm.json', bracket)
    require(pre_host['boot'] == hs['boot'] and pre_host['connection'] == hs['connection']
            and pre_host['session'] == hs['session'] and pre_host['phase'] == 'idle',
            'host binding changed before arm')
    require(pre_pico['boot'] == ps['boot'] and int(pre_pico['gen'], 16) == int(ps['gen'], 10)
            and pre_pico['state'] == '4', 'Pico binding changed before host arm')
    arm_start = time.monotonic_ns()
    try:
        got = host(dict(op='arm', boot=hs['boot'], run=run, trigger='heartbeat',
                        baseline=baseline, expected_message=baseline + 1))
    except BaseException as problem:
        arm_end = time.monotonic_ns()
        result = dict(clock='CLOCK_MONOTONIC', arm_request_before_ns=arm_start,
                      arm_response_after_ns=arm_end, error=type(problem).__name__,
                      message=str(problem)[:240], host_after=None, pico_after=None)
        try:
            started = time.monotonic_ns()
            value = host({'op': 'status'})
            result['host_after'] = dict(value=value, request_before_ns=started,
                                        response_after_ns=time.monotonic_ns())
        except BaseException as read_error:
            result['host_after_error'] = str(read_error)[:240]
        if getattr(usb, 'trustworthy', True):
            try:
                started = time.monotonic_ns()
                value = one(usb, 'AP2 LIVE', 'ap2 live ')
                result['pico_after'] = dict(value=value, request_before_ns=started,
                                            response_after_ns=time.monotonic_ns())
            except BaseException as read_error:
                result['pico_after_error'] = str(read_error)[:240]
        ap.persist(Path(out) / 'baseline-arm-refusal.json', result)
        raise
    require(got['phase'] == 'armed' and got['run'] == run
            and got['connection'] == hs['connection'] and got['session'] == hs['session']
            and got['baseline_message'] == got['last_message'] == baseline
            and got['expected_message'] == baseline + 1, 'host origin arm mismatch')
    live = one(usb, 'AP2 LIVE', 'ap2 live ')
    ap.require_prepared(live, ps['boot'], hs['session'])
    after = one(usb, 'AP2 TRIGGER', 'ap2 trigger ')
    require(after == origin, 'PREPARED heartbeat/input changed origin')
    final = host({'op': 'status'})
    require(all(final[k] == got[k] for k in ['boot', 'run', 'phase', 'connection',
                'session', 'last_message', 'expected_message']), 'host preparation changed')
    ready = ident | dict(baseline=baseline, expected=baseline + 1,
                         generation=int(live['gen'], 16), caps=CAPS, ev1_sequence=ev1['seq'])
    ap.persist(Path(out) / 'ready.json', ready)
    return ready


def header_evidence(result, packets, target, ready):
    """Bind the existing owned-socket observer output to this run, without payload."""
    binding = target['binding']
    require(binding['boot'] == ready['host_boot'] and binding['run'] == ready['run']
            and binding['connection'] == ready['host_connection']
            and binding['session'] == ready['session'] and binding['target'] == ready['device'],
            'header target binding mismatch')
    workers = [x for x in result.get('metadata_events', []) if x.get('event') == 'worker_finished']
    return result | dict(scope_valid=len(workers) == 1 and workers[0]['packets'] == len(packets)
                         and all(p['scope_valid'] for p in packets),
                         target_binding={k: ready[k] for k in
                             ['pico_boot', 'host_boot', 'run', 'device', 'host_connection', 'session']})


def observe(set_dtr, begin_headers, end_headers, *, wait=time.sleep,
            max_wait=MAX_WAIT, clock_ns=time.monotonic_ns):
    """Callbacks belong to the same owner; no USB reads/writes in this interval.

    The target-only observer is already prepared. At most 36.1s with the current
    explicit 16s wait budget; no heartbeat is generated to force progress. Late
    triggers are caught by USB-overlap/window quality, never declared complete.
    """
    require(0 < max_wait <= MAX_WAIT, 'finite heartbeat wait bound')
    controller=dict(clock='CLOCK_MONOTONIC',start_request_before_ns=clock_ns(),
                    wait_budget_ns=int(max_wait*1000000000),errors=[])
    try:started=begin_headers()
    except BaseException:
        end_headers()
        raise
    controller['start_confirm_after_ns']=clock_ns()
    if not isinstance(started,dict)or started.get('event')!='started':
        end_headers()
        raise ValueError('observer START unconfirmed; DTR gate not opened')
    try:
        controller['DTR_low_before_ns']=clock_ns()
        controller['required_end_ns']=controller['DTR_low_before_ns']+int((max_wait+WINDOW+.1)*1000000000)
        set_dtr(False)
        controller['DTR_low_after_ns']=clock_ns()
        wait(max_wait + WINDOW + .1)
    finally:
        controller['stop_request_before_ns']=clock_ns()
        try:
            headers = end_headers()
            controller['finished_after_ns']=clock_ns()
        finally:
            set_dtr(True)
    return (headers or {})|dict(controller=controller)


def collect(usb, host, ready, out, headers):
    """Freeze once, retain all evidence, and return quality without releasing it."""
    root = Path(out)
    pb, hb, run = ready['pico_boot'], ready['host_boot'], ready['run']
    host(dict(op='freeze', boot=hb, run=run))
    ps = one(usb, 'AP2 STATUS', 'ap2 status ')
    require(ps['boot'] == pb and ps['run'] == run, 'Pico capture identity changed')
    if ps['state'] != '3':
        one(usb, f'AP2 FREEZE {pb} {run}', 'ap2 status ')
    pico = ap.read_pico(usb, pb, run)
    server = ap.read_host(host, hb, run)
    trigger = one(usb, 'AP2 TRIGGER', 'ap2 trigger ')
    status = one(usb, 'TX1 STATUS', 'tx1 status ')
    require(status['b'] == pb and status['r'] == run and status['active'] == '0', 'TX identity/state')
    require(0 <= int(status['n'], 16) <= 32, 'TX bound')
    rows = []
    for i in range(int(status['n'], 16)):
        row = one(usb, f'TX1 READ {pb} {run} {i:x}', 'tx1 row ')
        value = {k: int(v, 10 if k == 'rc' else 16) for k, v in row.items()}
        require(value['i'] == i, 'TX row order')
        rows.append(value)
    require(txlib.tx_digest(rows) == int(status['h'], 16), 'TX digest')
    require(one(usb, 'TX1 STATUS', 'tx1 status ') == status, 'TX changed while frozen')
    inner = cy.collect(usb, pb, run, root)
    ev1 = one(usb, 'EV1 STATUS', 'ev1 status ')
    extra = dict(trigger=trigger, tx_status=status, tx_rows=rows, SDK_inner=inner, EV1=ev1)
    for name, value in [('pico', pico), ('host', server), ('extra', extra), ('headers', headers)]:
        ap.persist(root / (name + '.json'), value)
    result = validate(pico, server, extra, ready, headers)
    ap.persist(root / 'quality.json', result.record())
    raw_hashes = headers.get('SHA256', {})
    require(set(raw_hashes) == {'headers.jsonl', 'recvq.jsonl'}, 'header raw evidence manifest')
    for name, expected_hash in raw_hashes.items():
        path = root / name
        require(path.stat().st_size <= 524288 and hashlib.sha256(path.read_bytes()).hexdigest() == expected_hash,
                'header raw evidence missing/changed; retain capture')
    return confirm_saved(root,ready)


def release(usb, host, ready, saved, out):
    """Only after all pages/digests are saved; a failed capture may also release."""
    require(saved['ready'] == ready, 'saved identity mismatch')
    root = Path(out)
    require(json.loads((root / 'saved.json').read_text()) == saved, 'saved manifest changed')
    checked=validate_directory(root)
    require(checked['evidence_confirmed'],'saved evidence changed/rejected: '+str(checked['errors']))
    declared=checked['release'];p=declared['pico'];h=declared['host']
    ps = one(usb, f'AP2 RELEASE {p["boot"]} {p["run"]} {p["digest"]:x}', 'ap2 status ')
    require(ps['state'] == '0', 'Pico release unconfirmed')
    hs = host(dict(op='release', **h))
    require(hs['phase'] == 'idle', 'host release unconfirmed')
    ap.persist(Path(out) / 'released.json', dict(pico=ps, host=hs, owner_must_close=True))


def validate(pico, host, extra, ready, headers):
    """Projection adapter: codec checks become facts; only domain rules assess them."""
    from observation_rules import assess, IntegrityFacts
    host_crc=0
    for row in host['records']:
        canonical={k:row[k] for k in ['us','event','connection','message','session','frame','kind','a','b']}
        host_crc=zlib.crc32(json.dumps(canonical,separators=(',',':')).encode(),host_crc)
    inner=extra['SDK_inner']
    checks=IntegrityFacts(ap.fnv(pico['records'])==int(pico['status']['digest'],16),
        host_crc==host['status']['digest'],txlib.tx_digest(extra['tx_rows'])==int(extra['tx_status']['h'],16),
        inner['caps']==cy.SCHEMA,cy.digest(inner['rows'])==int(inner['status']['h'],16))
    return assess(pico,host,extra,ready,headers,checks)


def confirm_saved(root,ready):
    from heartbeat_evidence import confirm_saved as common
    return common(root,ready)


def confirm_partial(root,identity,host_captured):
    from heartbeat_evidence import confirm_partial as common
    return common(root,identity,host_captured)


def validate_directory(root):
    from heartbeat_evidence import validate_directory as common
    return common(root)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    result=validate_directory(args.directory)
    print(json.dumps(report_value(result),ensure_ascii=False))
    raise SystemExit(0 if result['complete']else 1)
