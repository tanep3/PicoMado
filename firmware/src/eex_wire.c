#include "eex_wire.h"
#include <stdio.h>
#include <string.h>
typedef struct { const uint8_t *p; size_t n; } token;
static bool equal(token t,const char *s){return strlen(s)==t.n&&!memcmp(t.p,s,t.n);}
static bool identifier(token t,bool hex){
    if(hex?t.n!=32:(!t.n||t.n>32))return false;
    for(size_t i=0;i<t.n;i++){
        unsigned c=t.p[i];
        if(hex){if(!((c>='0'&&c<='9')||(c>='a'&&c<='f')))return false;}
        else if(!((c>='0'&&c<='9')||(c>='a'&&c<='z')||(c>='A'&&c<='Z')||c=='_'||c=='-'))return false;
    }
    return true;
}
static void copy(char *out,token t){memcpy(out,t.p,t.n);out[t.n]=0;}
static bool decimal(token t,unsigned lo,unsigned hi,unsigned *out){
    if(!t.n||t.n>5||(t.n>1&&t.p[0]=='0'))return false;
    unsigned v=0;
    for(size_t i=0;i<t.n;i++){if(t.p[i]<'0'||t.p[i]>'9')return false;v=v*10+(t.p[i]-'0');}
    if(v<lo||v>hi)return false;
    *out=v;return true;
}
static bool ipv4(token t,uint32_t *out){
    uint32_t ip=0;size_t at=0;
    for(unsigned i=0;i<4;i++){
        size_t begin=at;while(at<t.n&&t.p[at]!='.')at++;
        unsigned v;if(!decimal((token){t.p+begin,at-begin},0,255,&v)||(i==0&&(!v||v>223)))return false;
        ip=(ip<<8)|v;
        if(i<3){if(at==t.n)return false;at++;}else if(at!=t.n)return false;
    }
    *out=ip;return true;
}
static const char *stage_name(pm_eex_stage s){
    switch(s){
    case PM_EEX_NEGOTIATED:return "NEGOTIATED";case PM_EEX_TARGET_CONNECTED:return "TARGET_CONNECTED";
    case PM_EEX_TCP_FAILED:return "TCP_FAILED";case PM_EEX_AUTH_REQUIRED:return "AUTH_REQUIRED";
    case PM_EEX_PROTOCOL_ERROR:return "PROTOCOL_ERROR";case PM_EEX_TARGET_REJECTED:return "TARGET_REJECTED";
    case PM_EEX_TIMEOUT:return "TIMEOUT";default:return NULL;
    }
}
bool pm_eex_result_valid(pm_eex_status status,pm_eex_stage stage){
    return (status==PM_EEX_OK&&(stage==PM_EEX_NEGOTIATED||stage==PM_EEX_TARGET_CONNECTED))||
        (status==PM_EEX_FAIL&&stage>=PM_EEX_TCP_FAILED&&stage<=PM_EEX_TIMEOUT);
}
bool pm_eex_parse(const uint8_t *wire,size_t length,pm_eex_message *out){
    if(!length||length>PM_EEX_MAX_DATAGRAM)return false;
    if(wire[length-1]=='\n')length--;
    if(!length||wire[0]==' '||wire[length-1]==' ')return false;
    token t[10];size_t count=0,begin=0;
    for(size_t i=0;i<=length;i++){
        if(i<length&&(wire[i]<0x20||wire[i]>0x7e))return false;
        if(i==length||wire[i]==' '){
            if(i==begin||count==10)return false;
            t[count++]=(token){wire+begin,i-begin};begin=i+1;
        }
    }
    if(count<5||!equal(t[0],"EEX1")||!identifier(t[2],true)||!identifier(t[3],false))return false;
    pm_eex_message m={0};copy(m.request,t[2]);copy(m.client,t[3]);
    unsigned v;
    if(equal(t[1],"D")){
        if(count!=5||!equal(t[4],"socks5"))return false;
        m.type=PM_EEX_D;
    }else{
        if(!identifier(t[4],true))return false;
        copy(m.offer,t[4]);
        if(equal(t[1],"O")){
            if(count!=10||!identifier(t[5],true)||!equal(t[9],"socks5"))return false;
            m.type=PM_EEX_O;copy(m.generation,t[5]);m.source_host=equal(t[6],"-");
            if(!m.source_host&&!ipv4(t[6],&m.host))return false;
            if(!decimal(t[7],1,65535,&v))return false;
            m.port=(uint16_t)v;
            if(!decimal(t[8],1,30,&v))return false;
            m.ttl=(uint8_t)v;
        }else if(equal(t[1],"A")){
            if(count!=5)return false;
            m.type=PM_EEX_A;
        }else if(equal(t[1],"R")){
            if(count!=8||!decimal(t[5],1,9,&v))return false;
            m.type=PM_EEX_R;m.attempt=(uint8_t)v;
            if(equal(t[6],"OK"))m.status=PM_EEX_OK;
            else if(equal(t[6],"FAIL"))m.status=PM_EEX_FAIL;else return false;
            bool found=false;
            for(unsigned s=0;s<=PM_EEX_TIMEOUT;s++)if(equal(t[7],stage_name((pm_eex_stage)s))){m.stage=(pm_eex_stage)s;found=true;break;}
            if(!found||!pm_eex_result_valid(m.status,m.stage))return false;
        }else if(equal(t[1],"K")){
            if(count!=7)return false;
            m.type=PM_EEX_K;
            if(equal(t[5],"A")&&equal(t[6],"0")){m.ack='A';m.attempt=0;}
            else if(equal(t[5],"R")&&decimal(t[6],1,9,&v)){m.ack='R';m.attempt=(uint8_t)v;}
            else return false;
        }else return false;
    }
    *out=m;return true;
}
static bool terminated(const char *s){return memchr(s,0,33)!=NULL;}
size_t pm_eex_encode(const pm_eex_message *m,char *out,size_t cap){
    if(!cap)return 0;
    out[0]=0;
    if(!terminated(m->request)||!terminated(m->client)||
        (m->type!=PM_EEX_D&&!terminated(m->offer))||
        (m->type==PM_EEX_O&&!terminated(m->generation)))return 0;
    int n=-1;char address[16];
    switch(m->type){
    case PM_EEX_D:n=snprintf(out,cap,"EEX1 D %s %s socks5\n",m->request,m->client);break;
    case PM_EEX_O:
        if(m->source_host)strcpy(address,"-");
        else snprintf(address,sizeof address,"%u.%u.%u.%u",(unsigned)(m->host>>24),
            (unsigned)((m->host>>16)&255),(unsigned)((m->host>>8)&255),(unsigned)(m->host&255));
        n=snprintf(out,cap,"EEX1 O %s %s %s %s %s %u %u socks5\n",m->request,m->client,m->offer,
            m->generation,address,(unsigned)m->port,(unsigned)m->ttl);break;
    case PM_EEX_A:n=snprintf(out,cap,"EEX1 A %s %s %s\n",m->request,m->client,m->offer);break;
    case PM_EEX_R:
        if(!pm_eex_result_valid(m->status,m->stage))return 0;
        n=snprintf(out,cap,"EEX1 R %s %s %s %u %s %s\n",m->request,m->client,m->offer,
            (unsigned)m->attempt,m->status==PM_EEX_OK?"OK":"FAIL",stage_name(m->stage));break;
    case PM_EEX_K:n=snprintf(out,cap,"EEX1 K %s %s %s %c %u\n",m->request,m->client,m->offer,m->ack,(unsigned)m->attempt);break;
    default:return 0;
    }
    if(n<0||(size_t)n>=cap||(size_t)n>PM_EEX_MAX_DATAGRAM){out[0]=0;return 0;}
    pm_eex_message checked;
    if(!pm_eex_parse((const uint8_t *)out,(size_t)n,&checked)){out[0]=0;return 0;}
    return (size_t)n;
}
