#include "endpoint_client.h"
#include <string.h>
static void close_io(pm_endpoint_client *c){if(c->opened)c->io.close(c->io.context);c->opened=false;}
static uint64_t increment(uint64_t n){n++;return n?n:1;}
void pm_endpoint_client_init(pm_endpoint_client *c,pm_endpoint_io io){
    memset(c,0,sizeof *c);c->io=io;c->network.wifi_generation=1;
    c->policy.mode=PM_ENDPOINT_EXCHANGE;c->policy.revision=1;c->discovery_port=48116;
}
void pm_endpoint_client_reset(pm_endpoint_client *c){
    close_io(c);c->enabled=false;c->probe_pending=false;c->failed=false;c->changed=false;c->cache.valid=false;
    c->network.wifi_generation=increment(c->network.wifi_generation);c->network.ipv4_ready=false;
    c->observed=(pm_endpoint_network){0};c->exchange=(pm_eex_exchange){.phase=PM_EEX_DONE};c->next_snapshot=0;
}
static void observe(pm_endpoint_client *c,uint64_t now){
    pm_endpoint_network n=c->io.network(c->io.context);
    bool different=n.ready!=c->observed.ready||n.ipv4!=c->observed.ipv4||
        n.gateway!=c->observed.gateway||n.netmask!=c->observed.netmask;
    if(different){
        if(c->observed.ready){c->changed=true;c->failed=true;}
        c->cache.valid=false;c->probe_pending=false;close_io(c);
        c->network.wifi_generation=increment(c->network.wifi_generation);
        c->exchange.phase=PM_EEX_EXPIRED;
    }
    c->observed=n;c->network.ipv4_ready=n.ready;c->network.local_ipv4=n.ipv4;
    c->network.gateway_ipv4=n.gateway;c->next_snapshot=now+250;
}
bool pm_endpoint_client_begin(pm_endpoint_client *c,const char *client,uint64_t now){
    c->enabled=true;observe(c,now);c->changed=false;c->failed=false;
    if(pm_endpoint_select(&c->network,&c->policy,&c->cache).status==PM_ENDPOINT_READY)return true;
    close_io(c);
    if(!c->network.ipv4_ready||!c->io.open(c->io.context,c->network.local_ipv4)){c->failed=true;return false;}
    c->opened=true;c->probe_pending=false;
    char nonce[33];static const char hex[]="0123456789abcdef";
    for(unsigned i=0;i<4;i++){
        uint32_t r=c->io.random(c->io.context);
        for(unsigned j=0;j<8;j++)nonce[i*8+j]=hex[(r>>(28-4*j))&15];
    }
    nonce[32]=0;c->request_generation=increment(c->request_generation);
    c->request=(pm_endpoint_request){c->network.wifi_generation,c->policy.revision,c->request_generation};
    if(!pm_eex_begin(&c->exchange,nonce,client,c->network.wifi_generation,now,120000)){
        c->failed=true;close_io(c);return false;
    }
    return true;
}
static void send_message(pm_endpoint_client *c,const pm_eex_message *m,pm_eex_peer to){
    size_t n=pm_eex_encode(m,c->tx,sizeof c->tx);
    if(n&&c->io.send(c->io.context,(const uint8_t *)c->tx,n,to))c->sent++;else c->send_errors++;
}
void pm_endpoint_client_poll(pm_endpoint_client *c,uint64_t now){
    if(!c->enabled)return;
    if(now>=c->next_snapshot)observe(c,now);
    if(!c->opened)return;
    // Receive callback does only bounded copy. Parse at most four packets on main.
    for(unsigned i=0;i<4;i++){
        pm_eex_peer from;size_t n=c->io.receive(c->io.context,c->rx,sizeof c->rx,&from);
        if(!n)break;
        c->received++;
        if(pm_eex_parse(c->rx,n,&c->incoming))pm_eex_receive(&c->exchange,&c->incoming,from,now);
    }
    unsigned jitter=1000;
    if(c->exchange.phase==PM_EEX_DISCOVERING&&now>=c->exchange.next_send)
        jitter=850+c->io.random(c->io.context)%301;
    pm_eex_action a=pm_eex_tick(&c->exchange,now,jitter);
    if(a.start_probe)c->probe_pending=true;
    if(a.send){
        if(a.message.type==PM_EEX_D){
            if(pm_endpoint_ipv4_unicast(c->network.gateway_ipv4))
                send_message(c,&a.message,(pm_eex_peer){c->network.gateway_ipv4,c->discovery_port});
            send_message(c,&a.message,(pm_eex_peer){UINT32_MAX,c->discovery_port});
        }else send_message(c,&a.message,c->exchange.reporter);
    }
    if(c->exchange.phase==PM_EEX_EXPIRED){c->failed=true;c->probe_pending=false;close_io(c);}
    else if(c->exchange.phase==PM_EEX_DONE)close_io(c);
}
bool pm_endpoint_client_take(pm_endpoint_client *c,uint64_t now,pm_endpoint *out){
    pm_endpoint_selection ready=pm_endpoint_select(&c->network,&c->policy,&c->cache);
    if(ready.status==PM_ENDPOINT_READY){*out=ready.endpoint;return true;}
    if(!c->probe_pending||c->exchange.phase!=PM_EEX_PROBING)return false;
    if(now>=c->exchange.offer_deadline){
        c->exchange.phase=PM_EEX_EXPIRED;c->probe_pending=false;c->failed=true;close_io(c);return false;
    }
    *out=(pm_endpoint){c->exchange.offer.source_host?c->exchange.reporter.ipv4:c->exchange.offer.host,
        c->exchange.offer.port};c->probe_pending=false;return true;
}
void pm_endpoint_client_result(pm_endpoint_client *c,pm_eex_status status,pm_eex_stage stage,uint64_t now){
    if(!pm_eex_probe_result(&c->exchange,c->network.wifi_generation,1,status,stage,now))return;
    if(c->exchange.verified){
        pm_endpoint e={c->exchange.offer.source_host?c->exchange.reporter.ipv4:c->exchange.offer.host,c->exchange.offer.port};
        (void)pm_endpoint_accept_offer(&c->network,&c->policy,&c->request,&c->request,e,true,&c->cache);
    }
}
bool pm_endpoint_client_network_changed(pm_endpoint_client *c){bool changed=c->changed;c->changed=false;return changed;}
