#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>

/* Read-only optional metadata scanner. A failure loses correlation only.
 * It never changes the existing domain parser's result or state. */
static inline void pm_oj_space(const char **p){while(**p==' '||**p=='\t'||**p=='\r'||**p=='\n')++*p;}
static inline int pm_oj_hex(char c){if(c>='0'&&c<='9')return c-'0';if(c>='a'&&c<='f')return c-'a'+10;if(c>='A'&&c<='F')return c-'A'+10;return -1;}
static inline bool pm_oj_string(const char **cursor,char *key){
 const char *p=*cursor;if(*p++!='"')return false;size_t used=0;
 while(*p && *p!='"'){
  unsigned c=(unsigned char)*p++;if(c<32)return false;
  if(c=='\\'){
   c=(unsigned char)*p++;if(!c)return false;
   if(c=='u'){
    unsigned value=0;for(unsigned i=0;i<4;i++){int digit=pm_oj_hex(*p);if(digit<0)return false;p++;value=value*16+(unsigned)digit;}
    c=value;
   }else if(c=='b')c=8;else if(c=='f')c=12;else if(c=='n')c=10;else if(c=='r')c=13;else if(c=='t')c=9;
   else if(c!='"'&&c!='\\'&&c!='/')return false;
  }
  if(key && used<15)key[used]=(c>0&&c<128)?(char)c:1;
  used++;
 }
 if(*p++!='"')return false;
 if(key){if(used>15){key[0]=1;key[1]=0;}else key[used]=0;}
 *cursor=p;return true;
}
static inline bool pm_oj_skip(const char **cursor,unsigned depth){
 if(depth>=16)return false;
 pm_oj_space(cursor);const char *p=*cursor;
 if(*p=='"'){if(!pm_oj_string(&p,NULL))return false;}
 else if(*p=='{'||*p=='['){
  bool object=*p++=='{';char close=object?'}':']';pm_oj_space(&p);
  if(*p!=close)for(;;){
   if(object){if(!pm_oj_string(&p,NULL))return false;pm_oj_space(&p);if(*p++!=':')return false;}
   if(!pm_oj_skip(&p,depth+1))return false;
   pm_oj_space(&p);
   if(*p!=',')break;
   p++;pm_oj_space(&p);
  }
  if(*p++!=close)return false;
 }else if(!strncmp(p,"true",4))p+=4;else if(!strncmp(p,"false",5))p+=5;else if(!strncmp(p,"null",4))p+=4;
 else {
  if(*p=='-')p++;
  if(*p=='0')p++;else{if(*p<'1'||*p>'9')return false;while(*p>='0'&&*p<='9')p++;}
  if(*p=='.'){p++;if(*p<'0'||*p>'9')return false;while(*p>='0'&&*p<='9')p++;}
  if(*p=='e'||*p=='E'){p++;if(*p=='+'||*p=='-')p++;if(*p<'0'||*p>'9')return false;while(*p>='0'&&*p<='9')p++;}
 }
 *cursor=p;return true;
}
static inline bool pm_oj_u64(const char **cursor,uint64_t *value){
 const char *p=*cursor;if(*p<'0'||*p>'9')return false;
 if(*p=='0'&&p[1]>='0'&&p[1]<='9')return false;
 uint64_t n=0;while(*p>='0'&&*p<='9'){unsigned digit=(unsigned)(*p++-'0');if(n>(UINT64_MAX-digit)/10)return false;n=n*10+digit;}
 *value=n;*cursor=p;return true;
}
static inline bool pm_oj_metadata(const char **cursor,uint64_t boot,uint64_t *operation){
 const char *p=*cursor;pm_oj_space(&p);if(*p++!='{')return false;unsigned seen=0;uint64_t version=0,source=0,op=0;
 for(;;){
  pm_oj_space(&p);char key[16];if(!pm_oj_string(&p,key))return false;pm_oj_space(&p);if(*p++!=':')return false;pm_oj_space(&p);
  unsigned bit=!strcmp(key,"version")?1:!strcmp(key,"boot")?2:!strcmp(key,"operation")?4:0;
  if(!bit||(seen&bit))return false;
  seen|=bit;
  uint64_t value;if(!pm_oj_u64(&p,&value))return false;
  if(bit==1)version=value;else if(bit==2)source=value;else op=value;
  pm_oj_space(&p);if(*p!=',')break;p++;
 }
 if(*p++!='}'||seen!=7||version!=1||!boot||source!=boot||!op)return false;
 *operation=op;*cursor=p;return true;
}
static inline bool pm_observation_json_cause(const char *p,uint64_t boot,uint64_t *operation){
 pm_oj_space(&p);if(*p++!='{')return false;bool found=false;uint64_t op=0;
 for(;;){
  pm_oj_space(&p);if(*p=='}')return false;
  char key[16];if(!pm_oj_string(&p,key))return false;pm_oj_space(&p);if(*p++!=':')return false;
  if(!strcmp(key,"observation")){if(found||!pm_oj_metadata(&p,boot,&op))return false;found=true;}
  else if(!pm_oj_skip(&p,1))return false;
  pm_oj_space(&p);if(*p=='}'){p++;break;}if(*p++!=',')return false;
 }
 pm_oj_space(&p);if(*p||!found)return false;*operation=op;return true;
}
