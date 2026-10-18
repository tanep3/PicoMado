#include "eex_wire.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static const struct { bool valid; const char *wire; } vectors[]={
    {true,"EEX1 D aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 socks5"},
    {true,"EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc 192.168.50.1 48115 30 socks5"},
    {true,"EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc - 1 1 socks5"},
    {true,"EEX1 A aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
    {true,"EEX1 R aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 1 OK NEGOTIATED"},
    {true,"EEX1 R aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 9 FAIL TARGET_REJECTED"},
    {true,"EEX1 K aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb A 0"},
    {true,"EEX1 K aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb R 2"},
    {false,"EEX2 D aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 socks5"},
    {false,"EEX1 D aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 http"},
    {false,"EEX1 D aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client! socks5"},
    {false,"EEX1  D aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 socks5"},
    {false,"EEX1 D abc client-1 socks5"},
    {false,"EEX1 D AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA client-1 socks5"},
    {false,"EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc 192.168.050.1 48115 30 socks5"},
    {false,"EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc 0.0.0.0 48115 30 socks5"},
    {false,"EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc 224.0.0.1 48115 30 socks5"},
    {false,"EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc - 65536 30 socks5"},
    {false,"EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc - 48115 31 socks5"},
    {false,"EEX1 R aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 0 OK NEGOTIATED"},
    {false,"EEX1 R aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 1 OK TCP_FAILED"},
    {false,"EEX1 R aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 1 FAIL NEGOTIATED"},
    {false,"EEX1 K aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb A 1"},
    {false,"EEX1 A aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb extra"},
};
int main(void){
    pm_eex_message m;char out[513],changed[1024];
    for(size_t i=0;i<sizeof vectors/sizeof *vectors;i++){
        size_t n=strlen(vectors[i].wire);
        bool ok=pm_eex_parse((const uint8_t *)vectors[i].wire,n,&m);
        if(ok!=vectors[i].valid){fprintf(stderr,"vector %zu failed\n",i);return 1;}
        if(ok){
            size_t sent=pm_eex_encode(&m,out,sizeof out);assert(sent==n+1);
            assert(!memcmp(out,vectors[i].wire,n)&&out[n]=='\n');
            assert(pm_eex_parse((const uint8_t *)out,sent,&m));
            assert(pm_eex_encode(&m,out,2)==0);
        }
    }
    const char *valid=vectors[0].wire;size_t n=strlen(valid);
    memcpy(changed,valid,n);changed[n]='\n';assert(pm_eex_parse((const uint8_t *)changed,n+1,&m));
    changed[n+1]='\n';assert(!pm_eex_parse((const uint8_t *)changed,n+2,&m));
    changed[n]='\r';changed[n+1]='\n';assert(!pm_eex_parse((const uint8_t *)changed,n+2,&m));
    memcpy(changed,valid,n);changed[4]='\t';assert(!pm_eex_parse((const uint8_t *)changed,n,&m));
    changed[4]=(char)0x80;assert(!pm_eex_parse((const uint8_t *)changed,n,&m));
    memcpy(changed,valid,n);changed[n]=' ';assert(!pm_eex_parse((const uint8_t *)changed,n+1,&m));
    changed[0]=0;assert(!pm_eex_parse((const uint8_t *)changed,n,&m));
    memset(changed,'a',513);assert(!pm_eex_parse((const uint8_t *)changed,513,&m));
    assert(!pm_eex_parse((const uint8_t *)"",0,&m));
    const char *loopback="EEX1 O aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa client-1 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccc 127.0.0.1 1 1 socks5";
    assert(pm_eex_parse((const uint8_t *)loopback,strlen(loopback),&m)&&m.host==0x7f000001);
    memset(m.request,'a',sizeof m.request);assert(!pm_eex_encode(&m,out,sizeof out));
    puts("PASS EEX1: 24 reconstructed Android vectors, canonical roundtrip, strict framing, loopback codec, bounded output");
}
