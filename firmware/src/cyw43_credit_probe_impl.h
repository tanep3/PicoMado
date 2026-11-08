/* Included only by the pinned private LL translation unit. */
#include "cyw43_credit_probe.h"
#include <string.h>
static pm_credit_row pm_credit_rows[PM_CREDIT_CAP];
static uint32_t pm_credit_count,pm_credit_dropped;
typedef struct {pm_credit_row *row;uint8_t prefix[64];} pm_credit_call;
static void pm_credit_inc(uint32_t *n){if(*n!=UINT32_MAX)++*n;}
bool pm_credit_get(unsigned i,pm_credit_row *out,uint32_t *count,uint32_t *dropped){
 *count=pm_credit_count;*dropped=pm_credit_dropped;
 if(i>=pm_credit_count||!pm_credit_rows[i].complete)return false;
 *out=pm_credit_rows[i];return true;
}
static pm_credit_call pm_credit_begin(unsigned kind,size_t len,const uint8_t *buf,size_t capacity,uint8_t flow,uint8_t credit,uint8_t sequence,uint32_t now){
 pm_credit_call c={0};
 if(pm_credit_count>=PM_CREDIT_CAP){pm_credit_inc(&pm_credit_dropped);return c;}
 c.row=&pm_credit_rows[pm_credit_count++];
 *c.row=(pm_credit_row){.at_us=now,.kind=(uint8_t)kind,.length=(uint16_t)(len>UINT16_MAX?UINT16_MAX:len),.flow=flow,.credit=credit,.sequence=sequence};
 size_t n=capacity>12?capacity-12:0;if(n>len)n=len;if(n>sizeof c.prefix)n=sizeof c.prefix;
 c.row->covered=(uint8_t)n;if(n)memcpy(c.prefix,buf+12,n);return c;
}
static void pm_credit_poll(pm_credit_call *c,int ret){
 if(!c->row)return;
 pm_credit_inc(&c->row->polls);
 if(ret==2)pm_credit_inc(&c->row->data);else if(ret==1)pm_credit_inc(&c->row->event);else if(ret==0)pm_credit_inc(&c->row->control);else if(ret<0)pm_credit_inc(&c->row->error);
}
static void pm_credit_ready(pm_credit_call *c,const uint8_t *buf,uint8_t flow,uint8_t credit,uint8_t sequence,uint32_t now){
 if(!c->row)return;
 c->row->wait_us=now-c->row->at_us;c->row->end_flow=flow;c->row->end_credit=credit;c->row->end_sequence=sequence;
 c->row->changed=c->row->covered&&memcmp(c->prefix,buf+12,c->row->covered)!=0;
 memset(c->prefix,0,sizeof c->prefix);
}
static void pm_credit_finish(pm_credit_call *c,int result,bool timeout){if(c->row){c->row->result=result;c->row->timeout=timeout;c->row->complete=true;}}
