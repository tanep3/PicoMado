#include "endpoint_client.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
typedef struct {
    pm_endpoint_network network;
    unsigned opens,closes,snapshots,sends;
    uint32_t random;
    bool open,packet;
    pm_eex_message incoming,last;
    pm_eex_peer from,to;
} mock;
static pm_endpoint_network network(void *v){mock *m=v;m->snapshots++;return m->network;}
static bool open_io(void *v,uint32_t local){mock *m=v;assert(local==m->network.ipv4&&!m->open);m->opens++;m->open=true;return true;}
static void close_io(void *v){mock *m=v;assert(m->open);m->closes++;m->open=false;}
static size_t receive_io(void *v,uint8_t *wire,size_t cap,pm_eex_peer *from){
    mock *m=v;if(!m->packet)return 0;m->packet=false;*from=m->from;
    return pm_eex_encode(&m->incoming,(char *)wire,cap);
}
static bool send_io(void *v,const uint8_t *wire,size_t len,pm_eex_peer to){
    mock *m=v;assert(m->open&&pm_eex_parse(wire,len,&m->last));m->to=to;m->sends++;return true;
}
static uint32_t random_io(void *v){return ++((mock *)v)->random;}
static void inject(mock *m,pm_eex_message packet){m->incoming=packet;m->packet=true;m->from=(pm_eex_peer){0x0a010201,49000};}
static pm_eex_message offer(pm_endpoint_client *c){
    pm_eex_message m=c->exchange.offer;m.type=PM_EEX_O;
    memset(m.offer,'b',32);m.offer[32]=0;memset(m.generation,'c',32);m.generation[32]=0;
    m.source_host=true;m.port=48115;m.ttl=30;return m;
}
static void accept(mock *m,pm_endpoint_client *c,uint64_t now){
    inject(m,offer(c));pm_endpoint_client_poll(c,now);
    assert(m->last.type==PM_EEX_A&&m->to.port==49000&&m->to.ipv4==0x0a010201);
    pm_eex_message k=c->exchange.offer;k.type=PM_EEX_K;k.ack='A';k.attempt=0;
    inject(m,k);pm_endpoint_client_poll(c,now+1);
}
int main(void){
    mock m={.network={true,0x0a010203,0x0a010201,0xffffff00}};
    pm_endpoint_io io={&m,network,open_io,close_io,receive_io,send_io,random_io};
    pm_endpoint_client c;pm_endpoint_client_init(&c,io);
    assert(pm_endpoint_client_begin(&c,"client-1",0));pm_endpoint_client_poll(&c,0);
    assert(m.sends==2&&m.last.type==PM_EEX_D&&m.to.ipv4==UINT32_MAX&&m.to.port==48116);
    pm_endpoint e;assert(!pm_endpoint_client_take(&c,0,&e));
    // User starts the app later. Discovery remains alive without blocking or joining Wi-Fi.
    for(uint64_t t=1000;t<=60000;t+=1000)pm_endpoint_client_poll(&c,t);
    assert(!c.failed&&m.opens==1&&m.sends<40);
    accept(&m,&c,60001);assert(pm_endpoint_client_take(&c,60002,&e));
    assert(e.ipv4==0x0a010201&&e.port==48115&&m.to.port!=e.port);
    assert(!pm_endpoint_client_take(&c,60003,&e)); // O duplication cannot schedule another connection.
    pm_endpoint_client_result(&c,PM_EEX_OK,PM_EEX_TARGET_CONNECTED,61000);
    pm_endpoint_client_poll(&c,61000);assert(m.last.type==PM_EEX_R&&c.cache.valid);
    // Notification app closes: lost R/K expires reporting only; verified endpoint survives.
    pm_endpoint_client_poll(&c,62500);assert(!m.open);
    unsigned opens=m.opens,sends=m.sends;
    assert(pm_endpoint_client_begin(&c,"client-1",3600000));
    assert(pm_endpoint_client_take(&c,3600000,&e)&&m.opens==opens&&m.sends==sends);
    uint64_t epoch=c.network.wifi_generation;
    pm_endpoint_client_poll(&c,3600250);assert(c.cache.valid&&c.network.wifi_generation==epoch);
    // DHCP prefix/gateway/address changes independently invalidate the observation.
    m.network.netmask=0xffff0000;pm_endpoint_client_poll(&c,3600500);
    assert(pm_endpoint_client_network_changed(&c)&&!pm_endpoint_client_take(&c,3600500,&e));
    assert(pm_endpoint_client_begin(&c,"client-1",3600501));accept(&m,&c,3600502);
    assert(pm_endpoint_client_take(&c,3600503,&e));
    pm_endpoint_client_result(&c,PM_EEX_FAIL,PM_EEX_TCP_FAILED,3600504);
    pm_endpoint_client_poll(&c,3600504);assert(m.last.status==PM_EEX_FAIL&&!c.cache.valid);
    pm_endpoint_client_poll(&c,3602004);assert(!m.open);
    char old[33];memcpy(old,c.exchange.offer.request,sizeof old);
    assert(pm_endpoint_client_begin(&c,"client-1",3603000)&&strcmp(old,c.exchange.offer.request));
    m.network.gateway=0x0a010209;pm_endpoint_client_poll(&c,3603250);
    assert(c.failed&&pm_endpoint_client_network_changed(&c)&&!m.open);
    assert(pm_endpoint_client_begin(&c,"client-1",3604000));
    m.network.ipv4=0x0a010204;pm_endpoint_client_poll(&c,3604250);
    assert(c.failed&&pm_endpoint_client_network_changed(&c));
    pm_endpoint_client_reset(&c);sends=m.sends;unsigned snapshots=m.snapshots;
    pm_endpoint_client_poll(&c,4000000);
    assert(!c.enabled&&!c.cache.valid&&m.sends==sends&&m.snapshots==snapshots); // HOME/standby has no work.
    assert(pm_endpoint_client_begin(&c,"client-1",4000000));
    pm_endpoint_client_poll(&c,4120000);assert(c.failed&&!m.open);
    assert(pm_endpoint_client_begin(&c,"client-1",5000000));
    pm_eex_message late=offer(&c);late.ttl=1;inject(&m,late);pm_endpoint_client_poll(&c,5000000);
    pm_endpoint_client_poll(&c,5001000);assert(c.failed&&!pm_endpoint_client_take(&c,5001000,&e));
    pm_endpoint_client_reset(&c);
    puts("PASS endpoint client mock IO: late app, gateway/broadcast, UDP tuple, TTL cache, TCP reuse, failed rediscovery, DHCP changes, HOME inactivity");
}
