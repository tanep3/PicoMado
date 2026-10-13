#include "ws_accept.h"
#include <stdint.h>
#include <string.h>

static uint32_t rol(uint32_t x,unsigned bits){return (x<<bits)|(x>>(32-bits));}
static void block(uint32_t h[5],const uint8_t *in){
    uint32_t w[80];
    for(unsigned i=0;i<16;i++)w[i]=(uint32_t)in[4*i]<<24|
        (uint32_t)in[4*i+1]<<16|(uint32_t)in[4*i+2]<<8|in[4*i+3];
    for(unsigned i=16;i<80;i++)w[i]=rol(w[i-3]^w[i-8]^w[i-14]^w[i-16],1);
    uint32_t a=h[0],b=h[1],c=h[2],d=h[3],e=h[4];
    for(unsigned i=0;i<80;i++){
        uint32_t f,k;
        if(i<20){f=(b&c)|(~b&d);k=0x5a827999u;}
        else if(i<40){f=b^c^d;k=0x6ed9eba1u;}
        else if(i<60){f=(b&c)|(b&d)|(c&d);k=0x8f1bbcdcu;}
        else{f=b^c^d;k=0xca62c1d6u;}
        uint32_t t=rol(a,5)+f+e+k+w[i];e=d;d=c;c=rol(b,30);b=a;a=t;
    }
    h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;h[4]+=e;
}
bool pm_ws_accept(const char *key,char out[29]){
    static const char guid[]="258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    size_t k=strlen(key);if(k!=24)return false;
    uint8_t input[128]={0};memcpy(input,key,k);memcpy(input+k,guid,sizeof guid-1);
    size_t n=k+sizeof guid-1;input[n]=0x80;
    uint64_t bits=n*8;
    for(unsigned i=0;i<8;i++)input[120+i]=(uint8_t)(bits>>(56-8*i));
    uint32_t h[5]={0x67452301u,0xefcdab89u,0x98badcfeu,0x10325476u,0xc3d2e1f0u};
    block(h,input);block(h,input+64);
    uint8_t digest[20];for(unsigned i=0;i<5;i++)for(unsigned j=0;j<4;j++)
        digest[i*4+j]=(uint8_t)(h[i]>>(24-8*j));
    static const char chars[]="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    unsigned pos=0;
    for(unsigned i=0;i<20;i+=3){
        uint32_t x=(uint32_t)digest[i]<<16;
        if(i+1<20)x|=(uint32_t)digest[i+1]<<8;
        if(i+2<20)x|=digest[i+2];
        out[pos++]=chars[(x>>18)&63];out[pos++]=chars[(x>>12)&63];
        out[pos++]=i+1<20?chars[(x>>6)&63]:'=';
        out[pos++]=i+2<20?chars[x&63]:'=';
    }
    out[pos]=0;return pos==28;
}
