#define PM_NET_TEST_OUTPUT_HOOK 1
#define PM_TX_ADAPTER_MAIN existing_adapter_cases
#include "tx_adapter_test.c"
static unsigned synchronous_calls;
static void synchronous_output(void){
    assert(lwip_depth==1);
    pm_ob_tx_range r=pm_ob_tx_find(&observed_net.tx,1,11);
    assert(r.copy&&r.notify==900); /* must already be visible inside tcp_output */
    assert(first.linkoutput(&first,&head)==original_result);
    synchronous_calls++;
}
static void flush_observation(void){for(unsigned i=0;i<100;i++)pm_trace_flush();trace_used=0;}
static uint64_t hex_field(const char *line,unsigned byte,unsigned length){
    char word[17];assert(length<=8);memcpy(word,line+4+byte*2,length*2);word[length*2]=0;return strtoull(word,NULL,16);
}
static const char *last_event(unsigned event){
    const char *last=NULL;
    for(const char *p=trace_text;(p=strstr(p,"ev1 "));p+=4)if(hex_field(p,1,2)==event)last=p;
    assert(last);return last;
}
int main(void){
    existing_adapter_cases(); /* AP2 hook ownership/forwarding unchanged with EV1 built in */
    packet_init();netif_default=&first;first.linkoutput=original1;
    cyw43_arch_lwip_begin();tx_attach();cyw43_arch_lwip_end();
    stage=PM_NET_READY;socket_instance.snd_lbb=1;
    ap_state=PM_AP_RECORDING;ap_run=13;ap_deadline=UINT64_MAX;
    cyw43_arch_lwip_begin();tx_sync();cyw43_arch_lwip_end();assert(tx_probe.active);
    pm_observation_init(42);pm_observation_enable(true);
    flush_observation();test_output_hook=synchronous_output;original_result=-5;output_result=-7;
    unsigned before=calls1;
    assert(pm_net_send_observed(8,"{}",900,800,7)); /* output error retains COPY bool */
    assert(synchronous_calls==1&&calls1==before+1&&tx_probe.count==1);
    pm_ob_tx_range copy=pm_ob_tx_find(&observed_net.tx,1,11);assert(copy.copy);
    flush_observation();invoke(); /* later retransmission, same range */
    assert(calls1==before+2&&tx_probe.count==2&&pm_ob_tx_find(&observed_net.tx,1,11).copy==copy.copy);
    test_output_hook=NULL;write_result=-1;unsigned next=observed_net.tx.next;
    assert(!pm_net_send_observed(8,"{}",901,800,7));assert(observed_net.tx.next==next);
    write_result=0;flush_observation();
    uint64_t previous=observed_net.connection;
    pm_observation_enable(false);pm_observation_enable(true);
    cyw43_arch_lwip_begin();assert(observe_connection()!=previous&&!observed_net.tx.copied);cyw43_arch_lwip_end();
    flush_observation();pm_net_close();assert(!observed_net.connection&&!observed_net.tx.copied);
    pm_net_begin(&test_profile);stage=PM_NET_READY;flush_observation();
    uint8_t wire[64],kind;char decoded[64];const uint8_t payload[]={'{','}'};
    size_t n=binary_ws(wire,8,payload,sizeof payload);
    feed(wire,n);assert(pm_net_take(&kind,decoded,sizeof decoded)&&kind==8);
    assert(observed_net.last_take&&observed_net.rx.slots[0].operation);
    uint64_t take=observed_net.last_take;
    pm_net_observed_api(1234,567);assert(!observed_net.last_take);
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    const char *bridge=last_event(PM_OB_NET_API);
    assert(hex_field(bridge,28,8)==1234&&hex_field(bridge,36,8)==take&&hex_field(bridge,52,8)==567);
    assert(hex_field(last_event(PM_OB_NET_TAKE_BASE+8),36,8)==observed_net.rx.slots[0].operation);
    trace_used=0;
    /* Splitting one envelope across callbacks leaves its raw parent unknown. */
    feed(wire,7);assert(!pm_net_take(&kind,decoded,sizeof decoded));
    feed(wire+7,n-7);assert(pm_net_take(&kind,decoded,sizeof decoded));
    assert(observed_net.last_take>take);
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    assert(!hex_field(last_event(PM_OB_NET_TAKE_BASE+8),36,8));trace_used=0;pm_net_close();
    puts("PASS actual net COPY published before synchronous output, retransmission parent, AP2 single forward, failed COPY/reset and output-error bool preserved");
    return 0;
}
