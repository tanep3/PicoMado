#!/usr/bin/env python3
"""Generate a pinned private LL diagnostic or single-change SPI comparison. Never edits the shared SDK."""
from pathlib import Path
import argparse,hashlib,json,difflib
EXPECTED='7f4c93755b2ab911d18606652497b2f3cd08085ce63b3b1836171d375f806b10'
def generate(source, spi_real_size=False, standalone=False):
 if hashlib.sha256(source.encode()).hexdigest()!=EXPECTED:raise ValueError('installed cyw43_ll.c differs from reviewed source; refusing instrumentation')
 if standalone:
  if not spi_real_size:raise ValueError('standalone requires spi_real_size')
  before='return cyw43_write_bytes(self, WLAN_FUNCTION, 0, CYW43_WRITE_BYTES_PAD(size), buf);'
  after='return cyw43_write_bytes(self, WLAN_FUNCTION, 0, size, buf);'
  if source.count(before)!=1:raise ValueError('ambiguous SDK SPI length replacement')
  return source.replace(before,after)
 text=source
 def one(before,after):
  nonlocal text
  if text.count(before)!=1:raise ValueError('ambiguous SDK insertion: '+before[:70])
  text=text.replace(before,after)
 text='#include "cyw43_inner_probe.h"\nconst char pm_cyw43_inner_translation_unit[] = "cyw43-inner-v2:7f4c9375";\n'+text
 one('''        return CYW43_FAIL_FAST_CHECK(-CYW43_EINVAL);
    }

    cyw43_ll_bus_sleep''','''        pm_cyw_probe_reject();
        return CYW43_FAIL_FAST_CHECK(-CYW43_EINVAL);
    }

    uint32_t pm_ticket = pm_cyw_probe_open(kind, len, buf, sizeof(self->spid_buf), self->wlan_flow_control, self->wwd_sdpcm_last_bus_data_credit, self->wwd_sdpcm_packet_transmit_sequence_number);
    cyw43_ll_bus_sleep''')
 one('''        uint32_t start_us = cyw43_hal_ticks_us();
        uint32_t last_poke = start_us - 100000;''','''        pm_cyw_probe_wait(pm_ticket);
        uint32_t start_us = cyw43_hal_ticks_us();
        uint32_t last_poke = start_us - 100000;''')
 one('''            int ret = cyw43_ll_sdpcm_poll_device(self, &res_len, &res_buf);
            if (ret != -1)''','''            int ret = cyw43_ll_sdpcm_poll_device(self, &res_len, &res_buf);
            /* Negative returns may leave res_len/res_buf uninitialized. */
            pm_cyw_probe_poll(pm_ticket, ret, ret >= 0 ? res_len : 0, buf, sizeof(self->spid_buf));
            if (ret != -1)''')
 one('''                return CYW43_FAIL_FAST_CHECK(-CYW43_ETIMEDOUT);''','''                int pm_result = CYW43_FAIL_FAST_CHECK(-CYW43_ETIMEDOUT);
                pm_cyw_probe_finish(pm_ticket, pm_result, true);
                return pm_result;''')
 one('''    return cyw43_write_bytes(self, WLAN_FUNCTION, 0, CYW43_WRITE_BYTES_PAD(size), buf);''','''    pm_cyw_probe_bus(pm_ticket, buf, sizeof(self->spid_buf), self->wlan_flow_control, self->wwd_sdpcm_last_bus_data_credit, self->wwd_sdpcm_packet_transmit_sequence_number);
    int pm_result = cyw43_write_bytes(self, WLAN_FUNCTION, 0, CYW43_WRITE_BYTES_PAD(size), buf);
    pm_cyw_probe_finish(pm_ticket, pm_result, false);
    return pm_result;''')
 one('''    if (SDPCM_HEADER_LEN + 2 + sizeof(struct sdpcm_bdc_header_t) + len > sizeof(self->spid_buf)) {
        return CYW43_FAIL_FAST_CHECK(-CYW43_EINVAL);''','''    if (SDPCM_HEADER_LEN + 2 + sizeof(struct sdpcm_bdc_header_t) + len > sizeof(self->spid_buf)) {
        pm_cyw_probe_reject();
        return CYW43_FAIL_FAST_CHECK(-CYW43_EINVAL);''')
 one('''    cyw43_read_bytes(self, WLAN_FUNCTION, 0, sz_align, self->spid_buf + 4);''','''    int pm_read_result = cyw43_read_bytes(self, WLAN_FUNCTION, 0, sz_align, self->spid_buf + 4);
    pm_cyw_probe_read(sz_align, pm_read_result);''')
 one('''    int ret = cyw43_read_bytes(self, WLAN_FUNCTION, 0, bytes_pending, self->spid_buf);''','''    int ret = cyw43_read_bytes(self, WLAN_FUNCTION, 0, bytes_pending, self->spid_buf);
    pm_cyw_probe_read(bytes_pending, ret);''')
 one('    // copy in payload\n    if (is_pbuf) {', '    // copy in payload\n    size_t pm_copied = len;\n    if (is_pbuf) {')
 one('        pbuf_copy_partial((const struct pbuf *)buf, self->spid_buf + SDPCM_HEADER_LEN + 6, len, 0);', '        pm_copied = pbuf_copy_partial((const struct pbuf *)buf, self->spid_buf + SDPCM_HEADER_LEN + 6, len, 0);')
 one('    return cyw43_sdpcm_send_common(self, DATA_HEADER, 6 + len, self->spid_buf);', '    pm_cyw_probe_copied(len, pm_copied);\n    return cyw43_sdpcm_send_common(self, DATA_HEADER, 6 + len, self->spid_buf);')
 if spi_real_size:
  one('int pm_result = cyw43_write_bytes(self, WLAN_FUNCTION, 0, CYW43_WRITE_BYTES_PAD(size), buf);',
      'int pm_result = cyw43_write_bytes(self, WLAN_FUNCTION, 0, size, buf);')
 return text

def generate_credit(source):
 if hashlib.sha256(source.encode()).hexdigest()!=EXPECTED:raise ValueError('installed cyw43_ll.c differs from reviewed source')
 text=source
 def one(a,b):
  nonlocal text
  if text.count(a)!=1:raise ValueError('ambiguous credit insertion')
  text=text.replace(a,b)
 text='#include "cyw43_credit_probe_impl.h"\n'+text
 one('    // Wait until we are allowed to send', '    pm_credit_call pm_credit = {0};\n    // Wait until we are allowed to send')
 one('        uint32_t start_us = cyw43_hal_ticks_us();\n        uint32_t last_poke = start_us - 100000;', '        uint32_t start_us = cyw43_hal_ticks_us();\n        pm_credit = pm_credit_begin(kind, len, buf, sizeof(self->spid_buf), self->wlan_flow_control, self->wwd_sdpcm_last_bus_data_credit, self->wwd_sdpcm_packet_transmit_sequence_number, start_us);\n        uint32_t last_poke = start_us - 100000;')
 one('            if (ret != -1) {', '            pm_credit_poll(&pm_credit, ret);\n            if (ret != -1) {')
 ready='pm_credit_ready(&pm_credit, buf, self->wlan_flow_control, self->wwd_sdpcm_last_bus_data_credit, self->wwd_sdpcm_packet_transmit_sequence_number, cyw43_hal_ticks_us());'
 one('                return CYW43_FAIL_FAST_CHECK(-CYW43_ETIMEDOUT);', '                '+ready+'\n                pm_credit_finish(&pm_credit, -CYW43_ETIMEDOUT, true);\n                return CYW43_FAIL_FAST_CHECK(-CYW43_ETIMEDOUT);')
 one('    size_t size = SDPCM_HEADER_LEN + len;', '    if (pm_credit.row) '+ready+'\n    size_t size = SDPCM_HEADER_LEN + len;')
 one('    return cyw43_write_bytes(self, WLAN_FUNCTION, 0, CYW43_WRITE_BYTES_PAD(size), buf);', '    int pm_result = cyw43_write_bytes(self, WLAN_FUNCTION, 0, CYW43_WRITE_BYTES_PAD(size), buf);\n    pm_credit_finish(&pm_credit, pm_result, false);\n    return pm_result;')
 return text

def main():
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--spi-real-size',action='store_true');p.add_argument('--standalone',action='store_true');p.add_argument('--credit-probe',action='store_true');a=p.parse_args();
 if a.credit_probe and (a.spi_real_size or a.standalone):p.error('credit probe excludes other overlay modes')
 s=a.source.read_text();v=generate_credit(s) if a.credit_probe else generate(s,a.spi_real_size,a.standalone);a.output.parent.mkdir(parents=True,exist_ok=True)
 if not a.output.exists()or a.output.read_text()!=v:a.output.write_text(v)
 (a.output.parent/'cyw43_inner.diff').write_text(''.join(difflib.unified_diff(s.splitlines(True),v.splitlines(True),fromfile='SDK/cyw43_ll.c',tofile='private/cyw43_ll.c')))
 (a.output.parent/'source.json').write_text(json.dumps({'original':str(a.source),'original_SHA256':EXPECTED,'patched':str(a.output),'patched_SHA256':hashlib.sha256(v.encode()).hexdigest(),'spi_real_size':a.spi_real_size,'inner_probe':not a.standalone and not a.credit_probe,'credit_probe':a.credit_probe,'shared_SDK_modified':False},indent=2)+'\n')
if __name__=='__main__':main()
