"""Pure project capture rules over decoded fact DTOs; no transport/storage dependency."""
from dataclasses import dataclass
from observation_domain import MeasurementAssessment

@dataclass(frozen=True)
class IntegrityFacts:
    pico_digest: bool
    host_digest: bool
    tx_digest: bool
    cy_schema: bool
    cy_digest: bool
    pico_generation: bool
    tx_generation: bool
    cy_generation: bool

def coverage_ok(headers):
    try:
        c=headers['controller']
        if c['clock']!='CLOCK_MONOTONIC'or c.get('errors'):return False
        starts=[e for e in headers['metadata_events']if e.get('event')=='started']
        workers=[e for e in headers['metadata_events']if e.get('event')=='worker_finished']
        if len(starts)!=1 or len(workers)!=1:return False
        a=starts[0]['clock'];worker=workers[0]
        points=[c['start_request_before_ns'],a['mono_before_ns'],a['mono_after_ns'],
                c['start_confirm_after_ns'],c['DTR_low_before_ns'],c['DTR_low_after_ns'],
                c['required_end_ns'],c['stop_request_before_ns'],c['finished_after_ns']]
        if not all(type(v)is int and v>=0 for v in points)or points!=sorted(points):return False
        budget=c['wait_budget_ns']
        if not 0<budget<=16000000000:return False
        if c['required_end_ns']!=c['DTR_low_before_ns']+budget+20100000000:return False
        if worker.get('packet_stats',{}).get('kernel_packets')!=worker.get('packets'):return False
        if worker.get('reason')!='stop_command'or worker.get('capture_started')is not True:return False
        low=(c['stop_request_before_ns']-c['start_confirm_after_ns'])/1e9
        high=(c['finished_after_ns']-c['start_request_before_ns'])/1e9
        duration=worker['capture_seconds']
        return isinstance(duration,(int,float))and low-.001<=duration<=high+.001 and worker['max_queue_sample_gap_ns']<=200000000
    except(KeyError,TypeError,ValueError):return False


def assess(pico, host, extra, ready, headers, integrity):
    """Assess the explicitly selected periodic observation, never infer its kind from rows."""
    errors = []
    def ck(ok, why):
        if not ok:
            errors.append(why)
    ps, hs, tr = pico['status'], host['status'], extra['trigger']
    rows, tx, inner = pico['records'], extra['tx_rows'], extra['SDK_inner']
    ts = extra['tx_status']
    baseline, target = ready['baseline'], ready['expected']
    mixed = ready.get('observation_kind', 'natural_heartbeat') == 'natural_heartbeat_then_sim'
    ck(ready.get('observation_kind', 'natural_heartbeat') in
       ('natural_heartbeat', 'natural_heartbeat_then_sim'), 'observation kind')
    ck(0 <= baseline < 0xffffffff and target == baseline + 1, 'ordinal origin')
    ck(ps['boot'] == tr['b'] == ready['pico_boot'] and ps['run'] == tr['r'] == ready['run'], 'Pico identity')
    ck(tr['mode'] == '1' and int(tr['o'], 16) == int(tr['g'], 16) == baseline
       and int(tr['m'], 16) == target and int(tr['flags'], 16) == (1 if mixed else 0), 'gate/target provenance')
    ck(ps['state'] == '3' and int(ps['over']) == 0 and integrity.pico_generation, 'Pico quality/generation')
    ck(integrity.pico_digest, 'Pico digest')
    ck(not any(pico[k] for k in ['prepared_input_detected', 'observation_usb_overlap_detected', 'post_freeze_buttons_detected']), 'USB/input quality')
    ck(not any(r['event'] == 1 or (not mixed and r['event'] == 24) for r in rows), 'unexpected input')
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
        if mixed:
            sim_rows = [r for r in rows if r['event'] == 24]
            ck(len(sim_rows) == 1 and rows.index(sim_rows[0]) > rows.index(output)
               if sim_rows else False, 'SIM after target heartbeat output')
    if mixed:
        try:
            sim = extra['SIM']; status = sim['status']; bind = sim['bind']
            step = sim['step']; synth = sim['synth']; job = ready['sim_job']
            sim_rows = [r for r in rows if r['event'] == 24]
            ck(len(sim_rows) == 1 and int(synth['n'], 16) == 1
               and synth['boot'] == ready['pico_boot'] and synth['run'] == ready['run']
               and int(synth['job'], 16) == job and int(synth['step'], 16) == 0,
               'SIM captured origin')
            ck(bind['b'] == bind['rb'] == status['b'] == ready['pico_boot']
               and bind['r'] == ready['run'] and int(bind['j'], 16) == int(status['j'], 16) == job
               and int(bind['s'], 16) == ready['session'], 'SIM binding')
            ck(status['phase'] == '4' and status['n'] == '1' and
               int(status['a'], 16) == int(status['d'], 16) == int(status['r'], 16) == 1
               and int(status['c'], 16) == int(status['why']) == 0,
               'SIM accepted/dispatched/released')
            ck(int(step['j'], 16) == job and step['i'] == '0' and
               step['a'] == step['d'] == step['r'] == '1' and step['c'] == '0'
               and int(step['key']) == ready['sim_key'], 'SIM step')
            ck(len(sim_rows) == 1 and (sim_rows[0]['a'] | sim_rows[0]['b'] << 32) == job
               and sim_rows[0]['c'] == 0 and sim_rows[0]['d'] & 255 == int(step['logical'])
               and sim_rows[0]['session'] == ready['session'], 'SIM event identity')
        except (KeyError, TypeError, ValueError):
            ck(False, 'SIM saved evidence')
    ck(hs['boot'] == ready['host_boot'] and hs['run'] == ready['run'] and hs['phase'] == 'frozen', 'host identity')
    ck(hs.get('trigger_mode') == 'heartbeat' and hs.get('baseline_message') == baseline
       and hs.get('expected_message') == hs.get('trigger_message') == target
       and hs.get('hb_connection') == ready['host_connection'] and hs.get('hb_session') == ready['session'], 'host origin mode')
    ck(all(r['connection'] == ready['host_connection'] for r in host['records']), 'host binding changed')
    ck(hs['overwritten'] == hs['contended'] == hs['pre_overwritten'] == 0, 'host observation loss')
    ck(integrity.host_digest, 'host digest')
    ck(int(pico['window']['trigger'], 16) > 0 and int(pico['window']['freeze'], 16) - int(pico['window']['trigger'], 16) >= 20000000
       and hs['trigger_us'] > 0 and hs['freeze_us'] - hs['trigger_us'] >= 20000000, 'full separate20s windows')
    ck(ts['b'] == ready['pico_boot'] and ts['r'] == ready['run'] and integrity.tx_generation
       and ts['active'] == '0' and ts['ok'] == '1' and all(int(ts[k], 16) == 0 for k in ['over', 'bad', 'changed']), 'TX quality')
    ck(integrity.tx_digest, 'TX digest')
    ck(integrity.cy_schema and inner['status']['b'] == ready['pico_boot'] and inner['status']['r'] == ready['run']
       and integrity.cy_generation and inner['status']['active'] == '0', 'CY binding/schema')
    ck(inner['no_diagnostic_loss'] and not any(int(inner['status'][k], 16) for k in ['over', 'inc', 'changed'])
       and not any(int(inner['quality'][k], 16) for k in ['sat', 'depth', 'reject']), 'CY loss')
    ck(integrity.cy_digest, 'CY digest')
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
    ck(coverage_ok(headers),'observer does not cover controller window')
    return MeasurementAssessment(tuple(errors), dict(trigger=ready.get('observation_kind', 'natural_heartbeat'),
                input_count=sum(r['event'] in [1, 24] for r in rows),
                input_before_or_during=bool(int(tr['flags'], 16) & 1),
                full_CY_scope='all target tuple data, including retransmissions',
                clocks_subtracted=False, RF_departure_proven=False))

