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
    got = host(dict(op='arm', boot=hs['boot'], run=run, trigger='heartbeat',
                    baseline=baseline, expected_message=baseline + 1))
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
            max_wait=MAX_WAIT):
    """Callbacks belong to the same owner; no USB reads/writes in this interval.

    The target-only observer is already prepared. At most 36.1s with the current
    explicit 16s wait budget; no heartbeat is generated to force progress. Late
    triggers are caught by USB-overlap/window quality, never declared complete.
    """
    require(0 < max_wait <= MAX_WAIT, 'finite heartbeat wait bound')
    begin_headers()
    try:
        set_dtr(False)
        wait(max_wait + WINDOW + .1)
    finally:
        try:
            headers = end_headers()
        finally:
            set_dtr(True)
    return headers


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
    ap.persist(root / 'quality.json', result)
    raw_hashes = headers.get('SHA256', {})
    require(set(raw_hashes) == {'headers.jsonl', 'recvq.jsonl'}, 'header raw evidence manifest')
    for name, expected_hash in raw_hashes.items():
        path = root / name
        require(path.stat().st_size <= 524288 and hashlib.sha256(path.read_bytes()).hexdigest() == expected_hash,
                'header raw evidence missing/changed; retain capture')
    names = [n + '.json' for n in ['pico', 'host', 'extra', 'headers', 'quality']] + list(raw_hashes)
    hashes = {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in names}
    ap.persist(root / 'saved.json', dict(ready=ready, hashes=hashes, pico_digest=int(pico['status']['digest'], 16),
               host_digest=server['status']['digest'], quality=result, released=False))
    return result


def release(usb, host, ready, saved, out):
    """Only after all pages/digests are saved; a failed capture may also release."""
    require(saved['ready'] == ready, 'saved identity mismatch')
    root = Path(out)
    require(json.loads((root / 'saved.json').read_text()) == saved, 'saved manifest changed')
    require(all(hashlib.sha256((root / n).read_bytes()).hexdigest() == h
                for n, h in saved['hashes'].items()), 'saved evidence changed; retain capture')
    pb, hb, run = ready['pico_boot'], ready['host_boot'], ready['run']
    ps = one(usb, f'AP2 RELEASE {pb} {run} {saved["pico_digest"]:x}', 'ap2 status ')
    require(ps['state'] == '0', 'Pico release unconfirmed')
    hs = host(dict(op='release', boot=hb, run=run, digest=saved['host_digest']))
    require(hs['phase'] == 'idle', 'host release unconfirmed')
    ap.persist(Path(out) / 'released.json', dict(pico=ps, host=hs, owner_must_close=True))


def validate(pico, host, extra, ready, headers):
    """No quality failure is waived for periodic mode; target ambiguity is fatal."""
    errors = []
    def ck(ok, why):
        if not ok:
            errors.append(why)
    ps, hs, tr = pico['status'], host['status'], extra['trigger']
    rows, tx, inner = pico['records'], extra['tx_rows'], extra['SDK_inner']
    ts = extra['tx_status']
    baseline, target = ready['baseline'], ready['expected']
    ck(0 <= baseline < 0xffffffff and target == baseline + 1, 'ordinal origin')
    ck(ps['boot'] == tr['b'] == ready['pico_boot'] and ps['run'] == tr['r'] == ready['run'], 'Pico identity')
    ck(tr['mode'] == '1' and int(tr['o'], 16) == int(tr['g'], 16) == baseline
       and int(tr['m'], 16) == target and int(tr['flags'], 16) == 0, 'gate/target provenance')
    ck(ps['state'] == '3' and int(ps['over']) == 0 and int(ps['gen']) == ready['generation'], 'Pico quality/generation')
    ck(ap.fnv(rows) == int(ps['digest'], 16), 'Pico digest')
    ck(not any(pico[k] for k in ['prepared_input_detected', 'observation_usb_overlap_detected', 'post_freeze_buttons_detected']), 'USB/input quality')
    ck(not any(r['event'] in [1, 24] for r in rows), 'unexpected input')
    ev1 = extra.get('EV1', {})
    ck(ev1.get('b') == ready['pico_boot'] and ev1.get('on') == ev1.get('sat') == '0'
       and ev1.get('seq') == ready.get('ev1_sequence') is not None, 'EV1 OFF throughout owned interval')
    markers = [r for r in rows if r['event'] == 25]
    ck(len(markers) == 1 and bool(rows) and rows[0]['event'] == 25, 'unique periodic trigger')
    expected_records = []
    for event in [25, 12, 13, 14, 15, 16, 23, 17, 18]:
        matches = [r for r in rows if r['event'] == event and r['message'] == target and r['kind'] == 5]
        ck(len(matches) == 1, 'target boundary ' + str(event))
        expected_records.extend(matches[:1])
    if len(expected_records) == 9:
        marker, before, locked, encoded, wb, we, copy, output, returned = expected_records
        ck([rows.index(r) for r in expected_records] == sorted(rows.index(r) for r in expected_records), 'target boundary order')
        ck(all(r['connection'] == ready['generation'] and r['session'] == ready['session'] for r in expected_records), 'target session/generation')
        ck(marker['a'] == marker['b'] == baseline and marker['c'] == 0 and marker['d'] == ready['generation'], 'marker origin')
        ck(not wb['flags'] & 6 and wb['b'] == copy['a'] == copy['d'], 'initial outstanding data/ACK ambiguity')
        wire = before['b'] + 6 + (6 if before['b'] + 6 < 126 else 8)
        ck(we['a'] == 0 and returned['a'] == 1 and (copy['b'] - copy['a']) & 0xffffffff == wire, 'target COPY failed/range')
        decoded = [r for r in host['records'] if r['event'] == 'decoded']
        found = [r for r in decoded if r['message'] == target]
        ck(len(found) == 1 and found[0]['kind'] == 5 and found[0]['session'] == marker['session']
           and found[0]['a'] == before['a'] and found[0]['b'] == before['b'], 'host target ordinal/kind/CRC')
        ck(bool(decoded) and [r['message'] for r in decoded] == list(range(target, target + len(decoded))), 'host decoded prelude/gap/duplicate')
        ck(any(t['s'] == copy['a'] and t['e'] == copy['b'] for t in tx), 'target first TX unobserved')
    ck(hs['boot'] == ready['host_boot'] and hs['run'] == ready['run'] and hs['phase'] == 'frozen', 'host identity')
    ck(hs.get('trigger_mode') == 'heartbeat' and hs.get('baseline_message') == baseline
       and hs.get('expected_message') == hs.get('trigger_message') == target
       and hs.get('hb_connection') == ready['host_connection'] and hs.get('hb_session') == ready['session'], 'host origin mode')
    ck(all(r['connection'] == ready['host_connection'] for r in host['records']), 'host binding changed')
    ck(hs['overwritten'] == hs['contended'] == hs['pre_overwritten'] == 0, 'host observation loss')
    digest = 0
    for row in host['records']:
        canonical = {k: row[k] for k in ['us', 'event', 'connection', 'message', 'session', 'frame', 'kind', 'a', 'b']}
        digest = zlib.crc32(json.dumps(canonical, separators=(',', ':')).encode(), digest)
    ck(digest == hs['digest'], 'host digest')
    ck(int(pico['window']['trigger'], 16) > 0 and int(pico['window']['freeze'], 16) - int(pico['window']['trigger'], 16) >= 20000000
       and hs['trigger_us'] > 0 and hs['freeze_us'] - hs['trigger_us'] >= 20000000, 'full separate20s windows')
    ck(ts['b'] == ready['pico_boot'] and ts['r'] == ready['run'] and ts['gen'] == format(ready['generation'], 'x')
       and ts['active'] == '0' and ts['ok'] == '1' and all(int(ts[k], 16) == 0 for k in ['over', 'bad', 'changed']), 'TX quality')
    ck(txlib.tx_digest(tx) == int(ts['h'], 16), 'TX digest')
    ck(inner['caps'] == cy.SCHEMA and inner['status']['b'] == ready['pico_boot'] and inner['status']['r'] == ready['run']
       and inner['status']['gen'] == format(ready['generation'], 'x') and inner['status']['active'] == '0', 'CY binding/schema')
    ck(inner['no_diagnostic_loss'] and not any(int(inner['status'][k], 16) for k in ['over', 'inc', 'changed'])
       and not any(int(inner['quality'][k], 16) for k in ['sat', 'depth', 'reject']), 'CY loss')
    ck(cy.digest(inner['rows']) == int(inner['status']['h'], 16), 'CY digest')
    ck(len(tx) == int(ts['n'], 16) <= 32 and len(inner['rows']) == int(inner['status']['n'], 16) <= 15, 'record counts')
    used = set()
    for t in tx:
        matches = [i for i, parts in enumerate(inner['rows']) if
            (parts[1]['s'], parts[1]['e'], parts[1]['a'], parts[1]['ip'], parts[1]['len']) ==
            (t['s'], t['e'], t['a'], t['ip'], t['len']) and
            t['t'] <= parts[0]['t'] <= parts[0]['end'] <= t['t'] + t['dt']]
        ck(len(matches) == 1 and matches[0] not in used, 'TX/CY unique full join')
        used.update(matches)
    ck(len(used) == len(inner['rows']), 'unmatched CY rows')
    ck(headers.get('complete_interval') is True and headers.get('payload_copied_or_persisted') is False, 'header observer quality')
    ck(headers.get('scope_valid') is True and headers.get('target_binding') ==
       {k: ready[k] for k in ['pico_boot', 'host_boot', 'run', 'device', 'host_connection', 'session']}, 'header target scope')
    workers = [x for x in headers.get('metadata_events', []) if x.get('event') == 'worker_finished']
    ck(len(workers) == 1 and workers[0]['packet_stats']['kernel_drops'] == 0
       and workers[0]['sockets_closed'] and not workers[0]['payload_copied'] and not workers[0]['payload_persisted'], 'header loss/scope lifecycle')
    return dict(complete=not errors, errors=errors, trigger='natural_heartbeat',
                input_count=sum(r['event'] in [1, 24] for r in rows),
                input_before_or_during=bool(int(tr['flags'], 16) & 1),
                full_CY_scope='all target tuple data, including retransmissions',
                clocks_subtracted=False, RF_departure_proven=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    read = lambda name: json.loads((args.directory / (name + '.json')).read_text())
    print(json.dumps(validate(read('pico'), read('host'), read('extra'), read('ready'), read('headers')), ensure_ascii=False))
