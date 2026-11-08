#include "server_link_hint.h"
#include <string.h>
typedef struct {const char *first,*end;} span;
static void space(const char **p,const char *end){while(*p<end && (**p==' '||**p=='\t'||**p=='\r'||**p=='\n'))++*p;}
static int hex(char c){if(c>='0'&&c<='9')return c-'0';if(c>='a'&&c<='f')return c-'a'+10;if(c>='A'&&c<='F')return c-'A'+10;return -1;}
static bool string(const char **p,const char *end){
    if(*p==end || *(*p)++!='"')return false;
    while(*p<end){unsigned char c=(unsigned char)*(*p)++;if(c=='"')return true;if(c<32)return false;
        if(c=='\\'){if(*p==end)return false;c=(unsigned char)*(*p)++;
            if(c=='u'){for(unsigned n=0;n<4;n++)if(*p==end || hex(*(*p)++)<0)return false;}
            else if(!c || !strchr("\"\\/bfnrt",c))return false;
        }
    }return false;
}
static bool skip(const char **p,const char *end,unsigned depth){
    space(p,end);if(*p==end || depth>6)return false;
    char c=**p;
    if(c=='"')return string(p,end);
    if(c=='{' || c=='['){
        char close=c=='{'?'}':']';++*p;space(p,end);
        if(*p<end && **p==close){++*p;return true;}
        for(;;){
            if(c=='{'){if(!string(p,end))return false;
            space(p,end);if(*p==end || *(*p)++!=':')return false;}
            if(!skip(p,end,depth+1))return false;
            space(p,end);
            if(*p==end)return false;
            char separator=*(*p)++;if(separator==close)return true;if(separator!=',')return false;
            space(p,end);
        }
    }
    const char *word=c=='t'?"true":c=='f'?"false":c=='n'?"null":NULL;
    if(word){size_t n=strlen(word);if((size_t)(end-*p)<n || memcmp(*p,word,n))return false;*p+=n;return true;}
    if(c=='-'){++*p;if(*p==end)return false;}
    if(**p=='0')++*p;
    else {if(**p<'1'||**p>'9')return false;do{++*p;}while(*p<end && **p>='0'&&**p<='9');}
    if(*p<end && **p=='.'){++*p;const char *start=*p;while(*p<end && **p>='0'&&**p<='9')++*p;if(*p==start)return false;}
    if(*p<end && (**p=='e'||**p=='E')){++*p;if(*p<end && (**p=='+'||**p=='-'))++*p;
        const char *start=*p;while(*p<end && **p>='0'&&**p<='9')++*p;if(*p==start)return false;}
    return true;
}
static bool text(span s,const char *expected){size_t n=strlen(expected);return (size_t)(s.end-s.first)==n+2 && s.first[0]=='"' && s.end[-1]=='"' && !memcmp(s.first+1,expected,n);}
/* Called only on an already validated object. Values remain bounded spans. */
static bool member(span object,const char **cursor,span *key,span *value){
    const char *p=*cursor?*cursor:object.first+1;space(&p,object.end);
    if(*p=='}')return false;
    key->first=p;(void)string(&p,object.end);key->end=p;space(&p,object.end);++p;space(&p,object.end);
    value->first=p;(void)skip(&p,object.end,0);value->end=p;space(&p,object.end);if(*p==',')++p;*cursor=p;return true;
}
static bool number(span s,uint64_t *out){
    if(s.first==s.end || *s.first<'0'||*s.first>'9' || (*s.first=='0'&&s.end-s.first!=1))return false;
    uint64_t value=0;for(const char *p=s.first;p<s.end;p++){
        if(*p<'0'||*p>'9')return false;
        unsigned digit=(unsigned)(*p-'0');if(value>(UINT64_MAX-digit)/10)return false;value=value*10+digit;
    }*out=value;return true;
}
static bool ipv4(span s,uint32_t *out){
    if(s.end-s.first<9 || s.end-s.first>17 || *s.first!='"'||s.end[-1]!='"')return false;
    const char *p=s.first+1,*end=s.end-1;uint32_t result=0;
    for(unsigned n=0;n<4;n++){
        const char *start=p;unsigned part=0;
        while(p<end && *p>='0'&&*p<='9'){part=part*10+(unsigned)(*p++-'0');if(part>255)return false;}
        if(p==start || p-start>3 || (p-start>1&&*start=='0'))return false;
        result=(result<<8)|part;if(n<3){if(p==end || *p++!='.')return false;}
    }if(p!=end)return false;
    *out=result;return true;
}
static bool mac(span s,uint8_t out[6]){
    if(s.end-s.first!=19 || *s.first!='"'||s.end[-1]!='"')return false;
    bool any=false;for(unsigned i=0;i<6;i++){
        int a=hex(s.first[1+i*3]),b=hex(s.first[2+i*3]);if(a<0||b<0||(i<5&&s.first[3+i*3]!=':'))return false;
        out[i]=(uint8_t)(a*16+b);any|=out[i]!=0;
    }return any && !(out[0]&1);
}
static pm_server_link_parse hint_parse(span s,pm_server_link_hint *out){
    if(s.end-s.first==4 && !memcmp(s.first,"null",4))return PM_SERVER_LINK_ABSENT;
    if(*s.first!='{')return PM_SERVER_LINK_INVALID;
    unsigned seen=0;const char *cursor=NULL;span key,value;pm_server_link_hint h={0};
    while(member(s,&cursor,&key,&value)){
        unsigned bit=0;bool ok=true;uint64_t v=0;
        if(text(key,"version")){bit=1;ok=number(value,&v)&&v==1;}
        else if(text(key,"server_ipv4")){bit=2;ok=ipv4(value,&h.server_ipv4);}
        else if(text(key,"client_ipv4")){bit=4;ok=ipv4(value,&h.client_ipv4);}
        else if(text(key,"prefix_length")){bit=8;ok=number(value,&v)&&v>=1&&v<=30;h.prefix_length=(uint8_t)v;}
        else if(text(key,"mac")){bit=16;ok=mac(value,h.mac);}
        if(!ok || (seen&bit))return PM_SERVER_LINK_INVALID;
        seen|=bit;
    }
    if(seen!=31)return PM_SERVER_LINK_INVALID;
    *out=h;return PM_SERVER_LINK_VALID;
}
bool pm_server_welcome_parse(const char *json,size_t length,pm_server_welcome *out){
    if(!json||!out||!length||length>PM_SERVER_LINK_WELCOME_MAX)return false;
    const char *p=json,*end=json+length;space(&p,end);span top={p,end};
    if(p==end||*p!='{'||!skip(&p,end,0))return false;
    top.end=p;space(&p,end);if(p!=end)return false;
    span key,value,data={0};const char *cursor=NULL;unsigned seen=0;
    while(member(top,&cursor,&key,&value)){
        if(text(key,"type")){if((seen&1)||!text(value,"welcome"))return false;seen|=1;}
        else if(text(key,"data")){if((seen&2)||*value.first!='{')return false;seen|=2;data=value;}
    }
    if(seen!=3)return false;
    pm_server_welcome result={0};seen=0;cursor=NULL;
    while(member(data,&cursor,&key,&value)){
        unsigned bit=0;
        if(text(key,"incarnation")){bit=1;if(!number(value,&result.incarnation)||!result.incarnation)return false;}
        else if(text(key,"session_epoch")){bit=2;if(!number(value,&result.session_epoch)||!result.session_epoch)return false;}
        else if(text(key,"media_input_version")){bit=4;if(!number(value,&result.media_input_version))result.media_input_version=0;}
        else if(text(key,"server_link")){bit=8;result.link=(seen&8)?PM_SERVER_LINK_INVALID:hint_parse(value,&result.hint);}
        if(bit!=8 && (seen&bit))return false;
        seen|=bit;
    }
    if((seen&3)!=3)return false;
    *out=result;return true;
}
uint32_t pm_server_link_ipv4(const uint8_t b[4]){return (uint32_t)b[0]<<24|(uint32_t)b[1]<<16|(uint32_t)b[2]<<8|b[3];}
unsigned pm_server_link_prefix(uint32_t mask){
    unsigned n=0;while(mask&0x80000000u){n++;mask<<=1;}return !mask&&n>=1&&n<=30?n:0;
}
bool pm_server_link_scope_equal(const pm_server_link_scope *a,const pm_server_link_scope *b){
    return a&&b&&a->connection==b->connection&&a->generation==b->generation&&a->netif_identity==b->netif_identity&&
        a->local_ipv4==b->local_ipv4&&a->peer_ipv4==b->peer_ipv4&&a->mask==b->mask&&a->profile==b->profile&&
        a->ready==b->ready&&a->up==b->up&&a->direct==b->direct;
}
static bool unicast(uint32_t ip){unsigned first=ip>>24;return first>0&&first<224&&first!=127;}
bool pm_server_link_admit(const pm_server_link_scope *q,const pm_server_link_scope *s,const pm_server_link_hint *h){
    if(!q||!s||!h||!pm_server_link_scope_equal(q,s)||!s->connection||!s->ready||!s->up||!s->direct||!s->netif_identity)return false;
    if(s->profile!=PM_PROFILE_HOME&&s->profile!=PM_PROFILE_HOME2)return false;
    if(!unicast(s->local_ipv4)||!unicast(s->peer_ipv4)||s->local_ipv4==s->peer_ipv4)return false;
    unsigned prefix=pm_server_link_prefix(s->mask);
    if(!prefix||prefix!=h->prefix_length||h->server_ipv4!=s->peer_ipv4||h->client_ipv4!=s->local_ipv4)return false;
    uint32_t net=s->local_ipv4&s->mask,hostmask=~s->mask;
    if((s->peer_ipv4&s->mask)!=net || !(s->local_ipv4&hostmask)||!(s->peer_ipv4&hostmask)||
       (s->local_ipv4&hostmask)==hostmask||(s->peer_ipv4&hostmask)==hostmask)return false;
    bool any=false;for(unsigned i=0;i<6;i++)any|=h->mac[i]!=0;return any&&!(h->mac[0]&1);
}
