#include "trace.h"
#include "ack_probe.h"
#include "net.h"
#include "core.h"
#include "stream_diag.h"
#include "ws_accept.h"
#include "rx_flow.h"
#include "socks5_wire.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "pico/stdlib.h"
#include "pico/rand.h"
#include "pico/cyw43_arch.h"
#include "lwip/ip_addr.h"
#include "lwip/tcp.h"
#if PM_TRACE_STATS
#include "lwip/stats.h"
#include "lwip/memp.h"
#endif

// One server image fragment is <17 KiB after JSON encoding. The TCP window
// must hold a whole fragment because receive credit is returned after parsing.
#define RX_CAP 32768u
// A 4096-byte JSON integer array needs at most four characters per byte.
// Reserve room for its envelope and WebSocket header before granting TCP credit.
_Static_assert(TCP_WND > 4096u * 4u + 512u, "TCP window must fit one image fragment");
_Static_assert(RX_CAP > TCP_WND, "receive buffer must exceed TCP window");
static struct tcp_pcb *pcb;
static const pm_profile *profile;
static pm_net_stage stage;
static enum { SOCKS_METHOD,SOCKS_AUTH,SOCKS_CONNECT } socks_step;
static uint8_t rx[RX_CAP];
static pm_rx_flow rx_flow;
#define rx_used rx_flow.used
// A legacy numeric-array chunk is <18KiB. Stream a WS body through this
// bounded application workspace; never require a whole concatenated WS body.
#define APP_CAP 18006u
_Static_assert(APP_CAP>4096u*4u+1024u,"legacy chunk workspace");
static uint8_t app_rx[APP_CAP];
static size_t ws_remaining;
static size_t app_used,last_payload_length;
static bool ws_continuation;
static bool socket_dead,ws_closed;
static uint64_t eof_started_us;
static bool proxy_target_connected;
static pm_net_failure last_failure;
static char expected_accept[29];
// The RP2040 main stack is 2 KiB. Keep large protocol workspaces in BSS.
static char ws_header[2049];
static uint8_t tx_packet[1040];
static char upgrade_request[320];
static uint8_t socks_auth_request[513];
static ip_addr_t server_addr, proxy_addr;
#if PM_EVENT_STREAM
#include "observation_transport.h"
static struct {
    uint64_t connection,capture,notify,parent,session,last_take;
    uint32_t received,consumed;
    bool overflow;
    pm_ob_tx_map tx;pm_ob_rx_map rx;pm_ob_app_map app;
} observed_net;
static uint64_t observe_connection(void){
    pm_observation_status status=pm_observation_read();
    if(!status.enabled){if(observed_net.connection)memset(&observed_net,0,sizeof observed_net);return 0;}
    if(observed_net.capture!=status.capture){memset(&observed_net,0,sizeof observed_net);observed_net.capture=status.capture;}
    if(!observed_net.connection){
        observed_net.connection=pm_observe(PM_OB_NETWORK,PM_OB_NET_CONNECTION,PM_OB_RECEIVED,0,0,0,0,(uint32_t)rx_used,(uint32_t)app_used);
        observed_net.received=(uint32_t)rx_used;observed_net.app.unknown=app_used!=0;
    }
    return observed_net.connection;
}
static uint64_t observe_copy(uint32_t first,uint32_t end){
    uint64_t connection=observe_connection();if(!connection)return 0;
    uint64_t copy=0;
    if(observed_net.notify){
        copy=pm_observe(PM_OB_NETWORK,PM_OB_NET_COPY,PM_OB_PENDING,0,observed_net.notify,connection,0,first,end);
        pm_observe(PM_OB_NETWORK,PM_OB_NET_WINDOW,PM_OB_PENDING,copy,observed_net.notify,connection,0,pcb->cwnd,pcb->snd_wnd);
        pm_observe(PM_OB_NETWORK,PM_OB_NET_POSITION,PM_OB_PENDING,copy,observed_net.notify,connection,0,pcb->snd_nxt,pcb->lastack);
    }
    /* Publish after COPY, before synchronous tcp_output/linkoutput. */
    pm_ob_tx_publish(&observed_net.tx,first,end,copy,observed_net.notify);
    return copy;
}
static void observe_rx(size_t n){
    uint64_t connection=observe_connection();if(!connection)return;
    if(n>UINT32_MAX-observed_net.received){observed_net.overflow=true;return;}
    uint32_t first=observed_net.received;observed_net.received+=(uint32_t)n;
    uint64_t op=pm_observe(PM_OB_NETWORK,PM_OB_NET_RX,PM_OB_RECEIVED,0,0,connection,0,first,observed_net.received);
    if(!observed_net.overflow)pm_ob_rx_publish(&observed_net.rx,first,observed_net.received,op);
}
void pm_net_observed_api(uint64_t operation,uint64_t frame){
    if(observed_net.last_take){
        pm_observe(PM_OB_NETWORK,PM_OB_NET_API,PM_OB_DISPATCHED,operation,observed_net.last_take,observed_net.connection,frame,0,0);
        observed_net.last_take=0;
    }
}
#endif
#if PM_TX_LINK_PROBE || PM_EVENT_STREAM
#include "lwip/netif.h"
#if PM_TX_LINK_PROBE
static pm_tx_probe tx_probe;
#endif
static struct netif *tx_netif;
static netif_linkoutput_fn tx_original;
static bool tx_read(const void *packet,unsigned offset,void *out,unsigned n){
    const struct pbuf *p=packet;
    return offset<=p->tot_len && n<=p->tot_len-offset && (!n || pbuf_copy_partial(p,out,(u16_t)n,(u16_t)offset)==n);
}
static int tx_forward(void *context,const void *packet){
    return tx_original((struct netif *)context,(struct pbuf *)packet);
}
static err_t tx_wrapper(struct netif *n,struct pbuf *p){
    /* SDK lock serializes callback writes with main snapshots/reset; no AP2 writer here. */
#if PM_EVENT_STREAM
    uint64_t operation=0,copy=0,connection=observe_connection();
    if(connection && pcb){
        pm_tx_tuple tuple;memcpy(tuple.local,ip_2_ip4(&pcb->local_ip),4);memcpy(tuple.peer,ip_2_ip4(&pcb->remote_ip),4);
        tuple.local_port=pcb->local_port;tuple.peer_port=pcb->remote_port;
        pm_tx_row key={0};
        if(pm_tx_packet_key(&tuple,tx_read,p,&key,NULL)){
            pm_ob_tx_range range=pm_ob_tx_find(&observed_net.tx,key.seq,key.end);copy=range.copy;
            operation=pm_observe(PM_OB_NETWORK,PM_OB_NET_LINK,PM_OB_DISPATCHED,0,copy,connection,0,key.seq,key.end);
        }
    }
#endif
#if PM_TX_LINK_PROBE
    unsigned refs=p->ref;
    err_t result=(err_t)pm_tx_observe(&tx_probe,tx_read,p,tx_forward,n,time_us_64,
        pcb?pcb->nrtx:0,refs>255?255:(uint8_t)refs);
#else
    err_t result=(err_t)tx_forward(n,p);
#endif
#if PM_EVENT_STREAM
    if(operation)pm_observe(PM_OB_NETWORK,PM_OB_NET_LINK,result==ERR_OK?PM_OB_COMPLETED:PM_OB_ERROR,operation,copy,connection,0,(uint32_t)result,pcb?pcb->nrtx:0);
#endif
    return result;
}
static void tx_detach(void){
    if(tx_netif && tx_netif->linkoutput==tx_wrapper)tx_netif->linkoutput=tx_original;
    tx_netif=NULL;tx_original=NULL;
#if PM_TX_LINK_PROBE
    tx_probe.attached=false;
    if(tx_probe.active)tx_probe.generation_changed=true;
    tx_probe.active=false;
#if PM_CYW43_INNER_PROBE
    pm_cyw_probe_sync(tx_probe.boot,tx_probe.run,tx_probe.generation,false,tx_probe.generation_changed,tx_probe.deadline_us,0,&tx_probe.tuple);
#endif
#endif
}
static void tx_attach(void){
    struct netif *n=netif_default;
    if(n==tx_netif && n && n->linkoutput==tx_wrapper)return;
    tx_detach();
    if(n && n->linkoutput && n->linkoutput!=tx_wrapper){tx_netif=n;tx_original=n->linkoutput;n->linkoutput=tx_wrapper;
#if PM_TX_LINK_PROBE
        tx_probe.attached=true;
#endif
    }
}
#if PM_TX_LINK_PROBE
static void tx_sync(void){
    uint64_t boot,run,deadline,frozen;unsigned state;uint32_t generation;
    bool recording=pm_ack_probe_capture(&boot,&run,&deadline,&state,&frozen,&generation);
    if(run && run!=tx_probe.run && state!=PM_AP_IDLE && state!=PM_AP_FROZEN){
        bool attached=tx_probe.attached;memset(&tx_probe,0,sizeof tx_probe);tx_probe.attached=attached;
        tx_probe.boot=boot;tx_probe.run=run;tx_probe.generation=generation;
        if(pcb){memcpy(tx_probe.tuple.local,ip_2_ip4(&pcb->local_ip),4);memcpy(tx_probe.tuple.peer,ip_2_ip4(&pcb->remote_ip),4);tx_probe.tuple.local_port=pcb->local_port;tx_probe.tuple.peer_port=pcb->remote_port;}
    }
    if(run && run==tx_probe.run && generation!=tx_probe.generation)tx_probe.generation_changed=true;
    if(state==PM_AP_FROZEN && frozen)while(tx_probe.count && tx_probe.rows[tx_probe.count-1].time_us>=frozen)tx_probe.count--;
    tx_probe.deadline_us=deadline;
    tx_probe.active=recording && run==tx_probe.run && !tx_probe.generation_changed && tx_probe.attached && time_us_64()<deadline;
#if PM_CYW43_INNER_PROBE
    pm_cyw_probe_sync(boot,run,generation,tx_probe.active,tx_probe.generation_changed,deadline,state==PM_AP_FROZEN?frozen:0,&tx_probe.tuple);
#endif
}
#if PM_CYW43_INNER_PROBE
void pm_net_cyw_status(pm_cyw_status *s){cyw43_arch_lwip_begin();tx_sync();pm_cyw_probe_status(s);cyw43_arch_lwip_end();}
bool pm_net_cyw_read(uint64_t boot,uint64_t run,unsigned index,pm_cyw_row *row){cyw43_arch_lwip_begin();tx_sync();bool ok=pm_cyw_probe_row(boot,run,index,row);cyw43_arch_lwip_end();return ok;}
#endif
void pm_net_tx_status(pm_tx_status *s){
    cyw43_arch_lwip_begin();tx_sync();
    *s=(pm_tx_status){.boot=tx_probe.boot,.run=tx_probe.run,.generation=tx_probe.generation,.count=tx_probe.count,.dropped=tx_probe.dropped,.malformed=tx_probe.malformed,.digest=pm_tx_digest(&tx_probe),.active=tx_probe.active,.attached=tx_probe.attached,.generation_changed=tx_probe.generation_changed};
    cyw43_arch_lwip_end();
}
bool pm_net_tx_read(uint64_t boot,uint64_t run,unsigned index,pm_tx_row *row){
    cyw43_arch_lwip_begin();tx_sync();
    bool ok=boot==tx_probe.boot && run==tx_probe.run && !tx_probe.active && index<tx_probe.count;
    if(ok)*row=tx_probe.rows[index];
    cyw43_arch_lwip_end();return ok;
}
#else
#define tx_sync() ((void)0)
#endif
#else
#define tx_sync() ((void)0)
#define tx_attach() ((void)0)
#define tx_detach() ((void)0)
#endif


#if PM_TRACE_STATS
// Caller already holds the SDK lock; no IO, allocation, or TCP state changes.
static pm_rx_state rx_observe(void){
    pm_rx_state s={.rx=(uint32_t)rx_used,.app=(uint32_t)app_used,.credit=(uint32_t)rx_flow.uncredited};
    if(pcb){
        s.tcp_valid=1;s.wnd=pcb->rcv_wnd;s.ann=pcb->rcv_ann_wnd;s.nxt=pcb->rcv_nxt;
#if TCP_QUEUE_OOSEQ
        s.ooseq=pcb->ooseq!=NULL;
#endif
    }
#if LWIP_STATS
    s.alloc.bits=sizeof(STAT_COUNTER)*8u;
#if MEMP_STATS
    if(lwip_stats.memp[MEMP_PBUF_POOL]){
        s.alloc.flags|=PM_RX_POOL;s.alloc.pool_err=lwip_stats.memp[MEMP_PBUF_POOL]->err;
        s.alloc.pool_used=lwip_stats.memp[MEMP_PBUF_POOL]->used;
    }
    if(lwip_stats.memp[MEMP_TCP_SEG]){
        s.alloc.flags|=PM_RX_SEG;s.alloc.seg_err=lwip_stats.memp[MEMP_TCP_SEG]->err;
        s.alloc.seg_used=lwip_stats.memp[MEMP_TCP_SEG]->used;
    }
#endif
#if MEM_STATS
    s.alloc.flags|=PM_RX_HEAP;s.alloc.heap_err=lwip_stats.mem.err;s.alloc.heap_used=lwip_stats.mem.used;
#endif
#if TCP_STATS
    s.alloc.flags|=PM_RX_TCP;s.alloc.tcp_memerr=lwip_stats.tcp.memerr;
    s.alloc.tcp_drop=lwip_stats.tcp.drop;s.alloc.tcp_chkerr=lwip_stats.tcp.chkerr;s.alloc.tcp_err=lwip_stats.tcp.err;
#endif
#endif
    return s;
}
#define RX_OBSERVE(call,...) do{pm_rx_state observed=rx_observe();call(__VA_ARGS__ &observed);}while(0)
#else
#define RX_OBSERVE(call,...) ((void)0)
#endif

static void consume(size_t n) {
#if PM_EVENT_STREAM
    bool tracked=observe_connection()!=0;
#endif
    size_t credit=0;
    if (!pm_rx_flow_consume(&rx_flow,n,&credit)) { RX_OBSERVE(pm_rx_fault,PM_RX_INVALID,0,); stage=PM_NET_FAILED; return; }
#if PM_EVENT_STREAM
    if(tracked){
        if(n>UINT32_MAX-observed_net.consumed)observed_net.overflow=true;
        else observed_net.consumed+=(uint32_t)n;
    }
#endif
    memmove(rx,rx+n,rx_used);
    if(pcb && credit)tcp_recved(pcb,(u16_t)credit);
    RX_OBSERVE(pm_rx_consume,(uint32_t)n,(uint32_t)credit,);
}
static uint16_t ack_probe_flags(void){
#if PM_ACK_PATH_PROBE
 return pcb?(uint16_t)(1u|(pcb->unsent?2u:0u)|(pcb->unacked?4u:0u)|((pcb->flags&TF_NODELAY)?8u:0u)):0;
#else
 return 0;
#endif
}
static bool send_raw(const void *p, size_t n) {
    if(!pcb){pm_ack_probe_net(PM_AP_WRITE_REJECT,0,1,(uint32_t)n,0,(uint32_t)stage);pm_logf("%s\n","net send_failed=no_socket");RX_OBSERVE(pm_rx_fault,PM_RX_SEND_ERROR,0,);stage=PM_NET_FAILED;return false;}
    if(n>tcp_sndbuf(pcb)){
        pm_ack_probe_net(PM_AP_WRITE_REJECT,ack_probe_flags(),2,(uint32_t)n,tcp_sndbuf(pcb),(uint32_t)stage);
        pm_logf("%s\n","net send_failed=send_buffer_full");RX_OBSERVE(pm_rx_fault,PM_RX_SEND_ERROR,0,);stage=PM_NET_FAILED;return false;
    }
    pm_ack_probe_net(PM_AP_WRITE_BEGIN,ack_probe_flags(),
#if PM_ACK_PATH_PROBE
        pcb->snd_nxt,pcb->lastack,pcb->cwnd,pcb->snd_wnd
#else
        0,0,0,0
#endif
    );
    uint64_t write_started=pm_trace_clock();
#if PM_EVENT_STREAM
    uint32_t copy_first=pcb->snd_lbb;
#endif
    err_t result=tcp_write(pcb,p,n,TCP_WRITE_FLAG_COPY);
    pm_ack_probe_net(PM_AP_WRITE_END,ack_probe_flags(),(uint32_t)result,tcp_sndbuf(pcb),
#if PM_ACK_PATH_PROBE
        pcb->snd_queuelen,pcb->snd_lbb
#else
        0,0
#endif
    );
    pm_trace_event(PM_TRACE_TCP_WRITE,pm_trace_current_frame(),result,
        (uint32_t)(pm_trace_clock()-write_started));
    if(result!=ERR_OK){
        pm_logf("net tcp_write_failed sdk_result=%d\n",(int)result);
        RX_OBSERVE(pm_rx_fault,PM_RX_SEND_ERROR,(int)result,);stage=PM_NET_FAILED; return false;
    }
#if PM_EVENT_STREAM
    uint64_t copy=observe_copy(copy_first,pcb->snd_lbb);
#endif
    pm_ack_probe_net(PM_AP_TX_POSITION,ack_probe_flags(),
#if PM_ACK_PATH_PROBE
        pcb->snd_lbb-(uint32_t)n,pcb->snd_lbb,pcb->snd_nxt,pcb->lastack
#else
        0,0,0,0
#endif
    );
    uint64_t output_started=pm_trace_clock();
    err_t output_result=tcp_output(pcb);
#if PM_EVENT_STREAM
    if(copy)pm_observe(PM_OB_NETWORK,PM_OB_NET_OUTPUT,output_result==ERR_OK?PM_OB_COMPLETED:PM_OB_ERROR,
        copy,observed_net.notify,observed_net.connection,0,(uint32_t)output_result,
        pcb?(1u|(pcb->unsent?2u:0u)|(pcb->unacked?4u:0u)|((pcb->flags&TF_NODELAY)?8u:0u)):0);
#endif
    pm_ack_probe_net(PM_AP_OUTPUT_END,ack_probe_flags(),(uint32_t)output_result,
#if PM_ACK_PATH_PROBE
        pcb->snd_nxt,pcb->lastack,tcp_sndbuf(pcb)
#else
        0,0,0
#endif
    );
    pm_trace_event(PM_TRACE_TCP_OUTPUT,pm_trace_current_frame(),output_result,
        (uint32_t)(pm_trace_clock()-output_started));
    // Preserve existing queueing semantics; output result is diagnostic, not arrival ACK.
    return true;
}
// A Close reply is optional: sndbuf measures bytes, not lwIP's segment queue
// or heap. Skip resource shortages once; never retry them or reset a failure.
static bool send_close_reply(const void *p,size_t n){
    if(!pcb || socket_dead){stage=PM_NET_FAILED;return false;}
    if(n>tcp_sndbuf(pcb))return true;
    uint64_t started=pm_trace_clock();
    err_t result=tcp_write(pcb,p,n,TCP_WRITE_FLAG_COPY);
    pm_trace_event(PM_TRACE_TCP_WRITE,pm_trace_current_frame(),result,
        (uint32_t)(pm_trace_clock()-started));
    if(socket_dead || stage==PM_NET_FAILED || !pcb)return false;
    if(result==ERR_MEM)return true;
    if(result==ERR_OK){
        started=pm_trace_clock();result=tcp_output(pcb);
        pm_trace_event(PM_TRACE_TCP_OUTPUT,pm_trace_current_frame(),result,
            (uint32_t)(pm_trace_clock()-started));
        if(socket_dead || stage==PM_NET_FAILED || !pcb)return false;
        if(result==ERR_OK || result==ERR_MEM)return true;
    }
    pm_logf("net close_reply_failed sdk_result=%d\n",(int)result);
    RX_OBSERVE(pm_rx_fault,PM_RX_SEND_ERROR,(int)result,);stage=PM_NET_FAILED;return false;
}
static void error_cb(void *arg, err_t e) {
    pm_diag_net_error((uint32_t)e);
    (void)arg;last_failure=stage==PM_NET_TCP?PM_NET_FAIL_TCP:PM_NET_FAIL_PROTOCOL;pm_logf("net tcp_error sdk_result=%d\n",(int)e);pcb=NULL;socket_dead=true;
    stage=PM_NET_FAILED;
    RX_OBSERVE(pm_rx_fault,PM_RX_TCP_ERROR,(int)e,);
}
static err_t recv_body(void *arg, struct tcp_pcb *p, struct pbuf *buf, err_t e) {
    (void)arg;
    // close/begin detach old callbacks under the SDK lock. Also reject a
    // stale PCB defensively before touching the current connection's buffers.
    if(p!=pcb){if(buf)pbuf_free(buf);return ERR_OK;}
    if (!buf && e!=ERR_OK){
        pm_diag_net_error((uint32_t)e);last_failure=PM_NET_FAIL_PROTOCOL;
        pm_logf("net tcp_error sdk_result=%d\n",(int)e);
        RX_OBSERVE(pm_rx_fault,PM_RX_TCP_ERROR,(int)e,);socket_dead=true;stage=PM_NET_FAILED;return ERR_OK;
    }
    if (!buf) {
        pm_diag_net_error(1);pm_logf("%s\n","net tcp_peer_closed");RX_OBSERVE(pm_rx_fault,PM_RX_PEER_CLOSED,0,);
        if(e==ERR_OK && stage==PM_NET_READY){eof_started_us=time_us_64();stage=PM_NET_DRAINING;}
        else if(e!=ERR_OK || (stage!=PM_NET_DRAINING && stage!=PM_NET_DRAINED))socket_dead=true;
        return ERR_OK;
    }
    // A validated Close ends application input, even if TCP FIN is delayed.
    if(ws_closed && e==ERR_OK){pbuf_free(buf);return ERR_OK;}
    size_t n=buf->tot_len, credit=0;
    RX_OBSERVE(pm_rx_callback_begin,(uint32_t)n,);
    if (e!=ERR_OK || buf->tot_len>RX_CAP-rx_used) {
        pm_logf("net receive_failed sdk_result=%d used=%u incoming=%u\n",(int)e,(unsigned)rx_used,(unsigned)buf->tot_len);
        unsigned freed=pbuf_free(buf);(void)freed;
        RX_OBSERVE(pm_rx_callback_end,freed,0,);
        RX_OBSERVE(pm_rx_fault,e==ERR_OK?PM_RX_OVERFLOW:PM_RX_TCP_ERROR,(int)e,);
        last_failure=PM_NET_FAIL_PROTOCOL;socket_dead=true;stage=PM_NET_FAILED;return ERR_OK;
    }
    pbuf_copy_partial(buf,rx+rx_used,n,0);
#if PM_EVENT_STREAM
    observe_rx(n);
#endif
#if PM_TRACE_STATS
    unsigned freed=pbuf_free(buf);
#else
    pbuf_free(buf);
#endif
    if(!pm_rx_flow_append(&rx_flow,n,RX_CAP,TCP_WND,&credit)) {
        RX_OBSERVE(pm_rx_callback_end,freed,0,);
        RX_OBSERVE(pm_rx_fault,PM_RX_OVERFLOW,0,);
        last_failure=PM_NET_FAIL_PROTOCOL;socket_dead=true;stage=PM_NET_FAILED;return ERR_OK;
    }
    // The copied bytes are safe in RX_CAP. Hold only the final window-sized
    // reserve, so receiving a partial frame never depends on parsing it first.
    if(credit)tcp_recved(p,(u16_t)credit);
    RX_OBSERVE(pm_rx_callback_end,freed,(uint32_t)credit,);
    return ERR_OK;
}
static err_t recv_cb(void *arg,struct tcp_pcb *p,struct pbuf *buf,err_t e){
    uint64_t started=pm_stats_clock();
    err_t result=recv_body(arg,p,buf,e);
    pm_stats_note(PM_STAT_RX_CALLBACK,started);
    return result;
}
static bool upgrade(void) {
    char key[25];
    static const char b64[]="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    uint8_t seed[16];
    for(unsigned i=0;i<16;i+=4) {
        uint32_t r=get_rand_32();
        for(unsigned j=0;j<4;j++)seed[i+j]=(uint8_t)(r>>(24-8*j));
    }
    for(unsigned i=0,j=0;i<16;i+=3) {
        uint32_t x=(uint32_t)seed[i]<<16;
        if(i+1<16)x|=(uint32_t)seed[i+1]<<8;
        if(i+2<16)x|=seed[i+2];
        key[j++]=b64[(x>>18)&63]; key[j++]=b64[(x>>12)&63];
        key[j++]=i+1<16?b64[(x>>6)&63]:'=';
        key[j++]=i+2<16?b64[x&63]:'=';
    }
    key[24]=0;
    if(!pm_ws_accept(key,expected_accept))return false;
    int n=snprintf(upgrade_request,sizeof upgrade_request,
        "GET /device HTTP/1.1\r\nHost: %s:16153\r\nUpgrade: websocket\r\n"
        "Connection: Upgrade\r\nSec-WebSocket-Key: %s\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n",profile->server_ip,key);
    if(n<=0 || (size_t)n>=sizeof upgrade_request) return false;
    stage=PM_NET_WS; return send_raw(upgrade_request,(size_t)n);
}
static err_t connected_cb(void *arg, struct tcp_pcb *p, err_t e) {
    (void)arg;(void)p;
    if(e!=ERR_OK){last_failure=PM_NET_FAIL_TCP;pm_logf("net tcp_connect_failed sdk_result=%d\n",(int)e);RX_OBSERVE(pm_rx_fault,PM_RX_TCP_ERROR,(int)e,);socket_dead=true;return e;}
    if(!profile->socks) { if(!upgrade())socket_dead=true; return ERR_OK; }
    const uint8_t greeting[]={5,1,2};
    socks_step=SOCKS_METHOD;
    stage=PM_NET_SOCKS;
    if(!send_raw(greeting,sizeof greeting))socket_dead=true;
    return ERR_OK;
}
bool pm_net_init(void) {
    return cyw43_arch_init()==0;
}
static void close_unlocked(void) {
#if PM_EVENT_STREAM
    memset(&observed_net,0,sizeof observed_net);
#endif
    tx_detach();
    pm_diag_drain_error();
    pm_diag_connection(false);
    pm_diag_event(PM_D_CLOSE,0,0,0,0);
    RX_OBSERVE(pm_rx_closed,);
    if(pcb) {
        struct tcp_pcb *old=pcb; pcb=NULL;
        tcp_arg(old,NULL); tcp_recv(old,NULL); tcp_err(old,NULL);
        if(tcp_close(old)!=ERR_OK)tcp_abort(old);
    }
    stage=PM_NET_DOWN;socks_step=SOCKS_METHOD;
    pm_rx_flow_reset(&rx_flow); app_used=0; last_payload_length=0; ws_continuation=false; ws_remaining=0; socket_dead=false;ws_closed=false;eof_started_us=0;
}
static void begin_unlocked(const pm_profile *p) {
    close_unlocked(); profile=p;proxy_target_connected=false;last_failure=PM_NET_FAIL_NONE;
    if(!p || !p->server_ip || !ipaddr_aton(p->server_ip,&server_addr)) { stage=PM_NET_FAILED;return; }
    if(p->socks && (!p->proxy_ip || !ipaddr_aton(p->proxy_ip,&proxy_addr) ||
                    !p->proxy_user || !p->proxy_password ||
                    !p->proxy_user[0] || !p->proxy_password[0] ||
                    strlen(p->proxy_user)>255 || strlen(p->proxy_password)>255)) {
        stage=PM_NET_FAILED;return;
    }
    tx_attach();
    pcb=tcp_new_ip_type(IPADDR_TYPE_V4);
    if(!pcb){RX_OBSERVE(pm_rx_fault,PM_RX_ALLOC_ERROR,0,);stage=PM_NET_FAILED;return;}
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    // Small controls/status share the frame connection. Do not hold them
    // (or a pending piggyback ACK) behind an earlier unacknowledged write.
    tcp_nagle_disable(pcb);
#endif
    pm_diag_connection(true);
    RX_OBSERVE(pm_rx_connection,);
    pcb->so_options|=SOF_KEEPALIVE;
    pcb->keep_idle=30000;pcb->keep_intvl=5000;pcb->keep_cnt=3;
    tcp_arg(pcb,NULL);tcp_recv(pcb,recv_cb);tcp_err(pcb,error_cb);
    stage=PM_NET_TCP;
    if(tcp_connect(pcb,p->socks?&proxy_addr:&server_addr,
        p->socks?p->proxy_port:16153,connected_cb)!=ERR_OK){last_failure=PM_NET_FAIL_TCP;stage=PM_NET_FAILED;}
}

static void process_socks(void) {
    if(socks_step==SOCKS_METHOD){
        int result=socks5_method_result(rx,rx_used);
        if(result==SOCKS5_INCOMPLETE)return;
        pm_logf("net socks_method result=%d\n",result);
        if(result!=SOCKS5_OK){stage=PM_NET_FAILED;return;}
        consume(2);
        size_t n=0;
        if(!socks5_make_auth(profile->proxy_user,profile->proxy_password,
            socks_auth_request,sizeof socks_auth_request,&n)){stage=PM_NET_FAILED;return;}
        socks_step=SOCKS_AUTH;
        bool sent=send_raw(socks_auth_request,n);
        memset(socks_auth_request,0,n); // tcp_write used COPY; clear credential copy.
        if(!sent)stage=PM_NET_FAILED;
        return;
    }
    if(socks_step==SOCKS_AUTH){
        int result=socks5_auth_result(rx,rx_used);
        if(result==SOCKS5_INCOMPLETE)return;
        pm_logf("net socks_auth result=%d\n",result);
        if(result!=SOCKS5_OK){stage=PM_NET_FAILED;return;}
        consume(2);
        uint8_t req[10];
        const ip4_addr_t *v=ip_2_ip4(&server_addr);
        socks5_make_connect_ipv4((const uint8_t *)&v->addr,16153,req);
        socks_step=SOCKS_CONNECT;
        if(!send_raw(req,sizeof req))stage=PM_NET_FAILED;
        return;
    }
    size_t n=0;
    int result=socks5_connect_result(rx,rx_used,&n);
    if(result==SOCKS5_INCOMPLETE)return;
    pm_logf("net socks_connect result=%d\n",result);
    if(result!=SOCKS5_OK){last_failure=result==SOCKS5_REJECTED?PM_NET_FAIL_TARGET:PM_NET_FAIL_PROTOCOL;stage=PM_NET_FAILED;return;}
    proxy_target_connected=true;
    consume(n);if(!upgrade())stage=PM_NET_FAILED;
}
static void check_failure_unlocked(void){
    bool eof_expired=(stage==PM_NET_DRAINING || stage==PM_NET_DRAINED) && time_us_64()-eof_started_us>=PM_NET_EOF_DRAIN_US;
    if(socket_dead || eof_expired){
        if(!socket_dead && eof_expired)last_failure=PM_NET_FAIL_EOF_TIMEOUT;
        close_unlocked();stage=PM_NET_FAILED;return;
    }
}
static void poll_unlocked(void) {
    check_failure_unlocked();
    if(stage==PM_NET_FAILED)return;
    if(stage==PM_NET_SOCKS && rx_used)process_socks();
    if(stage==PM_NET_WS) {
        static const uint8_t end[]={"\r\n\r\n"};
        for(size_t i=0;i+4<=rx_used;i++) if(!memcmp(rx+i,end,4)) {
            if(rx_used<12 || memcmp(rx,"HTTP/1.1 101",12)) {stage=PM_NET_FAILED;return;}
            // Limit the header scan to the terminated HTTP header, not binary payload.
            if(i+4>=sizeof ws_header){stage=PM_NET_FAILED;return;}
            memcpy(ws_header,rx,i+4);ws_header[i+4]=0;
            bool accepted=false;
            const char *line=ws_header;
            while((line=strstr(line,"\r\n"))!=NULL){
                line+=2;
                static const char name[]="Sec-WebSocket-Accept:";
                size_t j=0;
                for(;j<sizeof name-1 && line[j];j++){
                    char c=line[j],want=name[j];
                    if(c>='A'&&c<='Z')c+=32;
                    if(want>='A'&&want<='Z')want+=32;
                    if(c!=want)break;
                }
                if(j==sizeof name-1){
                    const char *v=line+j;
                    while(*v==' '||*v=='\t')v++;
                    const char *eol=strstr(v,"\r\n");
                    accepted=eol && eol-v==28 && !strncmp(v,expected_accept,28);
                    break;
                }
            }
            if(!accepted){stage=PM_NET_FAILED;return;}
            consume(i+4);stage=PM_NET_READY;return;
        }
        if(rx_used>2048)stage=PM_NET_FAILED;
    }
}
static pm_net_stage state_unlocked(void){return stage;}

static bool send_unlocked(uint8_t kind, const char *json) {
    if(ws_closed || (stage!=PM_NET_READY && stage!=PM_NET_DRAINING && stage!=PM_NET_DRAINED) || !pcb)return false;
    size_t n=strlen(json);if(n>1024)return false;
    pm_ack_probe_encoded(kind,json);
    uint8_t *packet=tx_packet;size_t k=0;
    packet[k++]=0x82;
    size_t len=n+6;
    if(len<126)packet[k++]=0x80|(uint8_t)len;
    else {packet[k++]=0xfe;packet[k++]=(uint8_t)(len>>8);packet[k++]=(uint8_t)len;}
    uint32_t random=get_rand_32();
    uint8_t mask[4]={random>>24,random>>16,random>>8,random};
    memcpy(packet+k,mask,4);k+=4;
    uint8_t header[6]={(uint8_t)((n+2)>>24),(uint8_t)((n+2)>>16),
        (uint8_t)((n+2)>>8),(uint8_t)(n+2),1,kind};
    for(size_t i=0;i<6;i++)packet[k++]=header[i]^mask[i&3];
    for(size_t i=0;i<n;i++)packet[k++]=(uint8_t)json[i]^mask[(i+6)&3];
    return send_raw(packet,k);
}

// Close reasons use strict UTF-8: reject overlong forms, surrogates and >U+10FFFF.
static bool close_utf8(const uint8_t *p,size_t n){
    for(size_t i=0;i<n;){
        unsigned c=p[i++],need,lo=0x80,hi=0xbf;
        if(c<0x80)continue;
        if(c>=0xc2 && c<=0xdf)need=1;
        else if(c>=0xe0 && c<=0xef){need=2;if(c==0xe0)lo=0xa0;if(c==0xed)hi=0x9f;}
        else if(c>=0xf0 && c<=0xf4){need=3;if(c==0xf0)lo=0x90;if(c==0xf4)hi=0x8f;}
        else return false;
        if(n-i<need || p[i]<lo || p[i]>hi)return false;
        i++;
        for(unsigned k=1;k<need;k++,i++)if(p[i]<0x80 || p[i]>0xbf)return false;
    }
    return true;
}
static bool graceful_close(size_t length){
    if(length){
        unsigned code=(unsigned)rx[2]<<8|rx[3];
        // Support normal/going-away Close; protocol/error/reserved codes fail.
        if((code!=1000 && code!=1001) || !close_utf8(rx+4,length-2)){stage=PM_NET_FAILED;return false;}
    }
    // Reply at a complete WS boundary when queue space is available. Do not
    // wait for an ACK or let an optional close response extend the deadline.
    if(stage==PM_NET_READY)eof_started_us=time_us_64();
    size_t n=length?8:6;
    {
        uint8_t reply[8]={0x88,(uint8_t)(0x80|(length?2:0))};
        uint32_t random=get_rand_32();
        for(unsigned i=0;i<4;i++)reply[2+i]=(uint8_t)(random>>(24-8*i));
        if(length){reply[6]=rx[2]^reply[2];reply[7]=rx[3]^reply[3];}
        if(!send_close_reply(reply,n) || socket_dead || stage==PM_NET_FAILED)return false;
    }
    ws_closed=true;stage=PM_NET_DRAINED;
    // Data after Close and incomplete app/WS state are not application input.
    // Keep only the main-owned CRC-valid ready pixels until pacing/EOF budget.
    pm_rx_flow_reset(&rx_flow);app_used=last_payload_length=ws_remaining=0;ws_continuation=false;
    return false;
}

size_t pm_net_payload_length(void){return last_payload_length;}
static bool take_unlocked(uint8_t *kind, char *json, size_t cap) {
again:
    if(stage!=PM_NET_READY && stage!=PM_NET_DRAINING)return false;
    if(app_used>=4) {
        uint32_t n=(uint32_t)app_rx[0]<<24|(uint32_t)app_rx[1]<<16|
            (uint32_t)app_rx[2]<<8|app_rx[3];
        if(n<2 || n>sizeof app_rx-4 || (n>4096 && app_used>=6 && app_rx[5]!=13 && app_rx[5]!=18)) {
            stage=PM_NET_FAILED;return false;
        }
        if(app_used>=n+4) {
            if(app_rx[4]!=1 || n-2>=cap){stage=PM_NET_FAILED;return false;}
            if(app_rx[5]<1 || app_rx[5]>18){stage=PM_NET_FAILED;return false;}
            last_payload_length=n-2;*kind=app_rx[5];memcpy(json,app_rx+6,n-2);json[n-2]=0;
#if PM_EVENT_STREAM
            uint32_t first=0,end=0;uint64_t parent=0;
            bool known=observe_connection() && !observed_net.overflow && pm_ob_app_take(&observed_net.app,n+4,&first,&end);
            if(known)parent=pm_ob_rx_find(&observed_net.rx,first,end);
            observed_net.last_take=pm_observe(PM_OB_NETWORK,(uint16_t)(PM_OB_NET_TAKE_BASE+*kind),PM_OB_COMPLETED,
                0,parent,observed_net.connection,0,known?first:UINT32_MAX,known?end:UINT32_MAX);
#endif
            app_used-=n+4;memmove(app_rx,app_rx+n+4,app_used);
#if PM_EVENT_STREAM
            if(!app_used)memset(&observed_net.app,0,sizeof observed_net.app);
#endif
            return true;
        }
    }
    if(ws_remaining){
        size_t n=rx_used;if(n>ws_remaining)n=ws_remaining;
        if(n>sizeof app_rx-app_used)n=sizeof app_rx-app_used;
        if(!n)return false;
#if PM_EVENT_STREAM
        if(observe_connection() && !observed_net.overflow)pm_ob_app_append(&observed_net.app,observed_net.consumed,(uint32_t)n);
#endif
        memcpy(app_rx+app_used,rx,n);app_used+=n;ws_remaining-=n;consume(n);
        goto again;
    }
    if(rx_used<2){
        // With FIN latched no second header byte can arrive. Inspect only a
        // frame boundary; 0x88 inside an unfinished binary body is data.
        if(rx_used && stage==PM_NET_DRAINING && (rx[0]&15)==8)stage=PM_NET_FAILED;
        return false;
    }
    unsigned opcode=rx[0]&15;
    bool fin=(rx[0]&0x80)!=0;
    if(rx[1]&0x80) {stage=PM_NET_FAILED;return false;}
    uint64_t length=rx[1]&127;size_t h=2;
    if(opcode==8 && (!fin || (rx[0]&0x70) || length>125 || length==1)){stage=PM_NET_FAILED;return false;}
    if(length==126){if(rx_used<4)return false;length=((unsigned)rx[2]<<8)|rx[3];h=4;}
    else if(length==127){
        if(rx_used<10)return false;
        length=0;for(unsigned i=2;i<10;i++)length=length<<8|rx[i];h=10;
    }
    if(length>RX_CAP-10 || length+ h >RX_CAP) {stage=PM_NET_FAILED;return false;}
    if(opcode==8){
        if(!fin || (rx[0]&0x70) || h!=2 || length>125 || length==1){stage=PM_NET_FAILED;return false;}
        if(rx_used<h+length){if(stage==PM_NET_DRAINING)stage=PM_NET_FAILED;return false;}
        return graceful_close((size_t)length);
    }
    if(opcode==9 && rx_used<h+length)return false;
    if(opcode==9){
        // Server ping is not an application message. Reply with a masked pong.
        if(length>125){stage=PM_NET_FAILED;return false;}
        uint8_t pong[131];size_t p=0;pong[p++]=0x8a;pong[p++]=0x80|(uint8_t)length;
        uint32_t random=get_rand_32();
        uint8_t mask[4]={random>>24,random>>16,random>>8,random};
        memcpy(pong+p,mask,4);p+=4;
        for(size_t i=0;i<length;i++)pong[p++]=rx[h+i]^mask[i&3];
        send_raw(pong,p);consume(h+(size_t)length);return false;
    }
    if((opcode!=2 && opcode!=0) || (opcode==0 && !ws_continuation) ||
       (opcode==2 && ws_continuation)) {
        stage=PM_NET_FAILED;return false;
    }
    ws_continuation=!fin;
    consume(h);ws_remaining=(size_t)length;
    goto again;
}

// Background IRQ owns RX writes; main owns app/pixel decoding. Copy/consume
// transport buffers under the SDK lock, then release it before rendering.
void pm_net_close(void){cyw43_arch_lwip_begin();close_unlocked();cyw43_arch_lwip_end();}
void pm_net_begin(const pm_profile *p){
    pm_ack_probe_connection();cyw43_arch_lwip_begin();begin_unlocked(p);cyw43_arch_lwip_end();}
void pm_net_poll(void){
    uint64_t started=pm_trace_clock();cyw43_arch_poll();
    uint64_t waiting=pm_stats_clock();cyw43_arch_lwip_begin();pm_stats_note(PM_STAT_SDK_WAIT,waiting);
    tx_sync();
    poll_unlocked();
#if PM_TRACE_STATS
    if(pm_rx_sample_due()){pm_rx_state observed=rx_observe();pm_rx_sample(&observed);}
#endif
    pm_diag_drain_error();
    if(pm_diag_sample_due(started))pm_diag_sample((uint32_t)rx_used,(uint32_t)app_used,(uint32_t)rx_flow.uncredited,pcb?pcb->rcv_wnd:0,pcb?pcb->rcv_nxt:0);
    #if PM_ACK_PATH_PROBE
    if(pm_ack_probe_sample_due())pm_ack_probe_sample(ack_probe_flags(),pcb?pcb->snd_nxt:0,pcb?pcb->lastack:0,pcb?pcb->cwnd:0,pcb?pcb->snd_wnd:0);
#endif
    cyw43_arch_lwip_end();
    pm_stats_note(PM_STAT_NET_POLL,started);pm_trace_slow(PM_TRACE_SLOW_POLL,started,2);
}
pm_net_stage pm_net_state(void){cyw43_arch_lwip_begin();pm_net_stage result=state_unlocked();cyw43_arch_lwip_end();return result;}
pm_net_stage pm_net_checkpoint(void){
    cyw43_arch_lwip_begin();check_failure_unlocked();pm_net_stage result=stage;cyw43_arch_lwip_end();return result;
}
bool pm_net_send(uint8_t kind,const char *json){
    pm_ack_probe_before_lock(kind,json);
    uint64_t started=pm_stats_clock();cyw43_arch_lwip_begin();pm_stats_note(PM_STAT_SDK_WAIT,started);
    pm_ack_probe_locked(kind);tx_sync();bool ok=send_unlocked(kind,json);cyw43_arch_lwip_end();pm_ack_probe_send_return(kind,ok);pm_stats_note(PM_STAT_NET_SEND,started);return ok;
}
#if PM_EVENT_STREAM
bool pm_net_send_observed(uint8_t kind,const char *json,uint64_t notify,uint64_t parent,uint64_t session){
    pm_ack_probe_before_lock(kind,json);
    uint64_t started=pm_stats_clock();cyw43_arch_lwip_begin();pm_stats_note(PM_STAT_SDK_WAIT,started);
    pm_ack_probe_locked(kind);tx_sync();
    observe_connection();
    observed_net.notify=notify;observed_net.parent=parent;observed_net.session=session;
    bool ok=send_unlocked(kind,json);observed_net.notify=observed_net.parent=observed_net.session=0;
    cyw43_arch_lwip_end();pm_ack_probe_send_return(kind,ok);pm_stats_note(PM_STAT_NET_SEND,started);return ok;
}
#endif
bool pm_net_take(uint8_t *kind,char *json,size_t cap){
    uint64_t started=pm_stats_clock();cyw43_arch_lwip_begin();pm_stats_note(PM_STAT_SDK_WAIT,started);
    size_t before_rx=rx_used,before_app=app_used,before_ws=ws_remaining;
    bool ok=take_unlocked(kind,json,cap);
    // false can mean a consumed ping, not exhaustion. Require a no-progress
    // decoder pass; with EOF no missing header/body bytes can arrive later.
    if(!ok && stage==PM_NET_DRAINING && before_rx==rx_used &&
       before_app==app_used && before_ws==ws_remaining)stage=PM_NET_DRAINED;
    RX_OBSERVE(pm_rx_take,ok,);
    if(stage==PM_NET_FAILED){RX_OBSERVE(pm_rx_fault,PM_RX_INVALID,0,);}
    cyw43_arch_lwip_end();pm_stats_note(PM_STAT_RX_TAKE,started);return ok;
}

// Optional video telemetry must not turn send-buffer pressure into a retry.
bool pm_net_send_optional(uint8_t kind,const char *json){
    pm_ack_probe_before_lock(kind,json);
    uint64_t started=pm_stats_clock();cyw43_arch_lwip_begin();pm_stats_note(PM_STAT_SDK_WAIT,started);
    bool room=stage==PM_NET_READY && pcb && strlen(json)+14<=tcp_sndbuf(pcb);
    tx_sync();bool ok=room && send_unlocked(kind,json);cyw43_arch_lwip_end();pm_ack_probe_send_return(kind,ok);pm_stats_note(PM_STAT_NET_SEND,started);return ok;
}

#if PM_TRACE_STATS
void pm_net_rx_frame_start(uint64_t frame,uint64_t session){
    cyw43_arch_lwip_begin();pm_rx_state s=rx_observe();
    pm_stats_frame_start();pm_rx_frame_begin(frame,session,&s);cyw43_arch_lwip_end();
}
void pm_net_rx_frame_end(pm_rx_reason reason,int error){
    if(reason==PM_RX_COMPLETE)pm_stats_frame_done();
    cyw43_arch_lwip_begin();pm_rx_state s=rx_observe();pm_rx_finish(reason,error,&s);cyw43_arch_lwip_end();
}
void pm_net_rx_snapshot(void){
    cyw43_arch_lwip_begin();pm_rx_state s=rx_observe();pm_rx_sample(&s);cyw43_arch_lwip_end();
}
#endif

bool pm_net_proxy_target_connected(void){cyw43_arch_lwip_begin();bool connected=proxy_target_connected;cyw43_arch_lwip_end();return connected;}
pm_net_failure pm_net_last_failure(void){cyw43_arch_lwip_begin();pm_net_failure failure=last_failure;
    if(failure==PM_NET_FAIL_NONE && stage==PM_NET_FAILED)failure=PM_NET_FAIL_PROTOCOL;
    cyw43_arch_lwip_end();return failure;}
