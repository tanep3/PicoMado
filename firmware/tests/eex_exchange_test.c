#include "eex_exchange.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
#define RID "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
#define OID "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
#define GEN "cccccccccccccccccccccccccccccccc"
static const pm_eex_peer phone={0x0a010201,48116},other={0x0a010201,48117};
static pm_eex_message message(const char *wire){
    pm_eex_message m;assert(pm_eex_parse((const uint8_t *)wire,strlen(wire),&m));return m;
}
static pm_eex_message offer(void){
    return message("EEX1 O " RID " client-1 " OID " " GEN " - 48115 30 socks5");
}
static pm_eex_message ack(char kind){
    return kind=='A'?message("EEX1 K " RID " client-1 " OID " A 0"):
        message("EEX1 K " RID " client-1 " OID " R 1");
}
static pm_eex_exchange begin(uint64_t now){
    pm_eex_exchange s;assert(pm_eex_begin(&s,RID,"client-1",7,now,120000));return s;
}
static void open_probe(pm_eex_exchange *s,uint64_t now){
    pm_eex_message o=offer(),k=ack('A');
    assert(pm_eex_receive(s,&o,phone,now));assert(pm_eex_tick(s,now,1000).message.type==PM_EEX_A);
    assert(pm_eex_receive(s,&k,phone,now+1));assert(pm_eex_tick(s,now+1,1000).start_probe);
}
int main(void){
    pm_eex_exchange s=begin(0);
    pm_eex_action a=pm_eex_tick(&s,0,850);assert(a.send&&a.message.type==PM_EEX_D&&s.next_send==425);
    assert(!pm_eex_tick(&s,424,1000).send);assert(pm_eex_tick(&s,425,1150).send);
    pm_eex_message o=offer();assert(pm_eex_receive(&s,&o,phone,1000));
    assert(s.reporter.port==48116&&s.offer.port==48115&&s.offer.source_host);
    uint64_t deadline=s.offer_deadline;
    assert(pm_eex_receive(&s,&o,phone,1100)&&s.offer_deadline==deadline);
    assert(!pm_eex_receive(&s,&o,other,1100));
    o.offer[0]='d';assert(!pm_eex_receive(&s,&o,phone,1100));o=offer();
    o.generation[0]='d';assert(!pm_eex_receive(&s,&o,phone,1100));o=offer();
    o.request[0]='d';assert(!pm_eex_receive(&s,&o,phone,1100));o=offer();
    o.client[0]='x';assert(!pm_eex_receive(&s,&o,phone,1100));
    pm_eex_message k=ack('R');assert(!pm_eex_receive(&s,&k,phone,1100)&&!s.receipt_confirmed);
    a=pm_eex_tick(&s,1000,1000);assert(a.send&&a.message.type==PM_EEX_A);
    assert(!pm_eex_tick(&s,1299,1000).send);assert(pm_eex_tick(&s,1300,1000).send);
    // Lost receipt K does not prevent an independent SOCKS verification.
    a=pm_eex_tick(&s,2500,1000);assert(a.start_probe&&!s.receipt_confirmed);
    assert(!pm_eex_tick(&s,2501,1000).start_probe);
    o=offer();assert(pm_eex_receive(&s,&o,phone,2600));assert(s.phase==PM_EEX_PROBING);
    assert(!pm_eex_probe_result(&s,8,1,PM_EEX_OK,PM_EEX_TARGET_CONNECTED,2600));
    assert(!pm_eex_probe_result(&s,7,2,PM_EEX_OK,PM_EEX_TARGET_CONNECTED,2600));
    assert(!pm_eex_probe_result(&s,7,1,PM_EEX_OK,PM_EEX_TCP_FAILED,2600));
    assert(pm_eex_probe_result(&s,7,1,PM_EEX_OK,PM_EEX_TARGET_CONNECTED,2600)&&s.verified);
    a=pm_eex_tick(&s,2600,1000);assert(a.send&&a.message.type==PM_EEX_R&&a.message.attempt==1);
    k=ack('A');assert(!pm_eex_receive(&s,&k,phone,2601)&&!s.receipt_confirmed);
    k=ack('R');assert(!pm_eex_receive(&s,&k,other,2601));assert(pm_eex_receive(&s,&k,phone,2601));
    assert(s.phase==PM_EEX_DONE&&s.result_confirmed&&!s.receipt_confirmed);
    assert(!pm_eex_probe_result(&s,7,1,PM_EEX_FAIL,PM_EEX_TCP_FAILED,2700));
    assert(s.verified&&!pm_eex_tick(&s,100000,1000).send);

    s=begin(0);o=offer();o.ttl=1;assert(pm_eex_receive(&s,&o,phone,0));
    assert(!pm_eex_tick(&s,1000,1000).start_probe&&s.phase==PM_EEX_EXPIRED);
    s=begin(0);o=offer();o.ttl=1;assert(pm_eex_receive(&s,&o,phone,0));
    k=ack('A');assert(pm_eex_receive(&s,&k,phone,10));assert(pm_eex_tick(&s,10,1000).start_probe);
    // TTL limits exchange/probe START, not an already-started verification or validated RAM cache.
    assert(pm_eex_probe_result(&s,7,1,PM_EEX_OK,PM_EEX_TARGET_CONNECTED,1100));
    assert(s.verified&&s.phase==PM_EEX_DONE&&!pm_eex_tick(&s,1100,1000).send);

    s=begin(0);open_probe(&s,0);
    assert(pm_eex_probe_result(&s,7,1,PM_EEX_OK,PM_EEX_NEGOTIATED,10)&&!s.verified);
    assert(pm_eex_tick(&s,10,1000).send);assert(!pm_eex_tick(&s,1510,1000).send&&s.phase==PM_EEX_DONE);
    s=begin(0);open_probe(&s,0);
    assert(pm_eex_probe_result(&s,7,1,PM_EEX_FAIL,PM_EEX_TIMEOUT,10));
    a=pm_eex_tick(&s,10,1000);assert(a.send&&a.message.stage==PM_EEX_TIMEOUT&&!s.verified);
    assert(!pm_eex_begin(&s,RID,"client-1",0,0,120000));
    s=begin(0);assert(!pm_eex_tick(&s,120000,1000).send&&s.phase==PM_EEX_EXPIRED);
    s=begin(UINT64_C(0x100000000));open_probe(&s,UINT64_C(0x100000000));
    assert(pm_eex_probe_result(&s,7,1,PM_EEX_OK,PM_EEX_TARGET_CONNECTED,UINT64_C(0x100000001)));
    assert(s.verified);
    s=begin(0);o=offer();o.source_host=false;o.host=0x7f000001;
    assert(!pm_eex_receive(&s,&o,phone,0)); // codec loopback is permitted; a physical Pico candidate is not.
    puts("PASS EEX1 exchange: tuple/nonces, separate A/R, idempotence, fixed TTL, bounded retry, post-expiry probe result, 64bit time");
}
