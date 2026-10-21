#include "ack_probe.h"
#include "observation_values.h"
#if PM_ACK_PATH_PROBE
#include "core.h"
#include "trace.h"
#include "pico/stdlib.h"
#include <stdio.h>
#include <string.h>
#include <stdarg.h>
// Main-loop only. No IRQ/callback writer, heap allocation or hot-path IO.
static struct {
 pm_ack_probe_record records[PM_ACK_PROBE_CAP];
 pm_ack_probe_summary summary;
 uint64_t session,frame,next_sample,next_export,live_session,sim_job;
 uint32_t sim_count,sim_step;
 bool live_active,usb_connected;
 uint32_t message,connection,buttons,head,prepared_crc,prepared_length,read_from,read_count,read_row;
 struct {uint32_t origin,gate,message,flags;uint8_t mode;} trigger;
 uint8_t active_kind;
 bool reading,input_overflow;
 unsigned input_used;
 char input[96],reply[144],pending[144];
 size_t pending_length;
} probe;
static bool capture_transition(obs_pico_event event){
 obs_pico_state next;
 if(!obs_pico_next((obs_pico_state)probe.summary.state,event,&next))return false;
 probe.summary.state=(unsigned)next;
 return true;
}
_Static_assert((unsigned)PM_AP_IDLE==(unsigned)obs_pico_IDLE && (unsigned)PM_AP_ARMED==(unsigned)obs_pico_ARMED && (unsigned)PM_AP_RECORDING==(unsigned)obs_pico_RECORDING && (unsigned)PM_AP_FROZEN==(unsigned)obs_pico_FROZEN && (unsigned)PM_AP_PREPARED==(unsigned)obs_pico_PREPARED,"wire state values unchanged");
_Static_assert(sizeof(pm_ack_probe_record)==56,"fixed record format");
_Static_assert(sizeof probe<=6144,"probe BSS must fit 6KiB");
static bool tracked(uint8_t k){return k==1||k==5||k==6||k==7||k==8||k==9||k==14||k==15||k==16;}
static uint32_t digest(void){
 uint32_t h=2166136261u;
 for(unsigned i=0;i<probe.summary.count;i++){
  const uint8_t *p=(const uint8_t *)&probe.records[(probe.head+i)%PM_ACK_PROBE_CAP];
  for(unsigned j=0;j<sizeof(pm_ack_probe_record);j++)h=(h^p[j])*16777619u;
 }
 return h;
}
static void freeze(uint32_t reason,uint64_t now){
 if(probe.summary.state==PM_AP_FROZEN||probe.summary.state==PM_AP_IDLE)return;
 probe.summary.freeze_buttons=probe.buttons;(void)capture_transition(obs_pico_FREEZE);probe.summary.reason=reason;probe.summary.freeze_us=now;
 probe.summary.digest=digest();
}
static void expire(uint64_t now){
 if(probe.summary.state==PM_AP_RECORDING && now-probe.summary.trigger_us>=PM_ACK_PROBE_WINDOW_US)freeze(1,now);
}
static void append(pm_ack_probe_event event,uint8_t kind,uint32_t msg,uint64_t session,uint64_t frame,uint16_t flags,uint32_t a,uint32_t b,uint32_t c,uint32_t d){
 if(probe.summary.state!=PM_AP_RECORDING)return;
 uint64_t now=time_us_64();expire(now);if(probe.summary.state!=PM_AP_RECORDING)return;
 unsigned at=(probe.head+probe.summary.count)%PM_ACK_PROBE_CAP;
 if(probe.summary.count==PM_ACK_PROBE_CAP){probe.head=(probe.head+1)%PM_ACK_PROBE_CAP;if(probe.summary.dropped!=UINT32_MAX)probe.summary.dropped++;}
 else probe.summary.count++;
 probe.records[at]=(pm_ack_probe_record){.us=now,.session=session,.frame=frame,.connection=probe.connection,.message=msg,.a=a,.b=b,.c=c,.d=d,.event=event,.flags=flags,.kind=kind};
}
void pm_ack_probe_init_boot(uint64_t boot){memset(&probe,0,sizeof probe);probe.summary.boot=boot?boot:1;}
void pm_ack_probe_init(void){pm_ack_probe_init_boot(time_us_64());}
void pm_ack_probe_usb_state(bool connected){
 uint64_t now=time_us_64();expire(now);
 // A waiting run is PREPARED while USB is open. DTR never resets data.
 if(connected && probe.summary.state==PM_AP_ARMED)(void)capture_transition(obs_pico_OPEN_GATE);
 if(!connected && probe.usb_connected && probe.summary.state==PM_AP_PREPARED){
  (void)capture_transition(obs_pico_CLOSE_GATE);probe.summary.gate_us=now;probe.summary.gate_buttons=probe.buttons;
  probe.trigger.gate=probe.message;
  if(probe.trigger.mode&&probe.trigger.gate!=probe.trigger.origin)probe.trigger.flags|=2u;
 }
 if(connected && probe.summary.state==PM_AP_RECORDING && !probe.summary.usb_overlap_us)probe.summary.usb_overlap_us=now?now:1;
 probe.usb_connected=connected;
}

void pm_ack_probe_live(uint64_t session,bool active){probe.live_session=session;probe.live_active=active;}
void pm_ack_probe_context(uint64_t s,uint64_t f){probe.session=s;probe.frame=f;}
void pm_ack_probe_note(pm_ack_probe_event e,uint8_t k,uint64_t s,uint64_t f,uint32_t a,uint32_t b,uint32_t c,uint32_t d){append(e,k,0,s,f,0,a,b,c,d);}
void pm_ack_probe_button(uint64_t s,uint64_t display,uint64_t owner,unsigned b,bool long_press){
 uint64_t now=time_us_64();expire(now);probe.buttons++;
 if(probe.trigger.mode&&probe.summary.state!=PM_AP_IDLE&&probe.summary.state!=PM_AP_FROZEN)probe.trigger.flags|=1u;
  if(probe.summary.state==PM_AP_ARMED&&!probe.trigger.mode){(void)capture_transition(obs_pico_START);probe.summary.trigger_us=now;probe.next_sample=now;}
 append(PM_AP_BUTTON,0,0,s,0,0,b,long_press,0,0);
 append(PM_AP_CONTEXT,0,0,s,0,0,(uint32_t)display,(uint32_t)(display>>32),(uint32_t)owner,(uint32_t)(owner>>32));
}
void pm_ack_probe_sim_button(uint64_t session,uint64_t display,uint64_t owner,unsigned b,bool long_press,uint64_t job,unsigned step){
 uint64_t now=time_us_64();expire(now);
 if(probe.trigger.mode&&probe.summary.state!=PM_AP_IDLE&&probe.summary.state!=PM_AP_FROZEN)probe.trigger.flags|=1u;
  if(probe.summary.state==PM_AP_ARMED&&!probe.trigger.mode){(void)capture_transition(obs_pico_START);probe.summary.trigger_us=now;probe.next_sample=now;}
 if(probe.summary.state!=PM_AP_RECORDING)return;
 probe.sim_count++;probe.sim_job=job;probe.sim_step=step;
 append(PM_AP_SIM_BUTTON,0,0,session,0,0,(uint32_t)job,(uint32_t)(job>>32),step,(b&255u)|(long_press?256u:0));
 append(PM_AP_CONTEXT,0,0,session,0,0,(uint32_t)display,(uint32_t)(display>>32),(uint32_t)owner,(uint32_t)(owner>>32));
}
bool pm_ack_probe_capture(uint64_t *boot,uint64_t *run,uint64_t *deadline,unsigned *state,uint64_t *frozen,uint32_t *generation){
 *boot=probe.summary.boot;*run=probe.summary.run;*state=probe.summary.state;*generation=probe.connection;
 *deadline=probe.summary.trigger_us+PM_ACK_PROBE_WINDOW_US;*frozen=probe.summary.freeze_us;
 return probe.summary.state==PM_AP_RECORDING;
}
bool pm_ack_probe_usb_busy(void){return probe.reply[0]||probe.reading||probe.pending_length;}
void pm_ack_probe_before_lock(uint8_t k,const char *json){
 if(k==5 && probe.trigger.mode && probe.summary.state==PM_AP_ARMED && !probe.usb_connected && probe.live_active){
  if(probe.message==UINT32_MAX){probe.trigger.flags|=8u;return;}
  uint64_t now=time_us_64();(void)capture_transition(obs_pico_START);probe.summary.trigger_us=now;probe.next_sample=now;
  probe.trigger.message=probe.message+1;probe.session=probe.live_session;probe.frame=0;
  append(PM_AP_PERIODIC_TRIGGER,5,probe.trigger.message,probe.live_session,0,0,probe.trigger.origin,probe.trigger.gate,probe.trigger.flags,probe.connection);
 }
 probe.active_kind=0;probe.prepared_crc=probe.prepared_length=0;
 if(tracked(k)&&probe.summary.state==PM_AP_RECORDING){size_t n=strnlen(json,1025);if(n<=1024)probe.prepared_crc=pm_crc32((const uint8_t *)json,n);probe.prepared_length=(uint32_t)n;append(PM_AP_BEFORE_LOCK,k,probe.message+1,probe.session,probe.frame,0,probe.prepared_crc,probe.prepared_length,0,0);}
}
void pm_ack_probe_locked(uint8_t k){if(tracked(k))append(PM_AP_LOCKED,k,probe.message+1,probe.session,probe.frame,0,0,0,0,0);}
void pm_ack_probe_encoded(uint8_t k,const char *json){
 if(probe.trigger.mode&&probe.summary.state!=PM_AP_IDLE&&probe.summary.state!=PM_AP_FROZEN&&probe.message==UINT32_MAX)probe.trigger.flags|=8u;
 probe.message++;probe.active_kind=tracked(k)?k:0;(void)json;
 if(probe.active_kind)append(PM_AP_ENCODED,k,probe.message,probe.session,probe.frame,0,probe.prepared_crc,probe.prepared_length,0,0);
}
void pm_ack_probe_net(pm_ack_probe_event e,uint16_t flags,uint32_t a,uint32_t b,uint32_t c,uint32_t d){if(probe.active_kind)append(e,probe.active_kind,probe.message,probe.session,probe.frame,flags,a,b,c,d);}
void pm_ack_probe_send_return(uint8_t k,bool ok){if(tracked(k))append(PM_AP_SEND_RETURN,k,probe.message,probe.session,probe.frame,0,ok,0,0,0);probe.active_kind=0;}
void pm_ack_probe_connection(void){if(probe.trigger.mode&&probe.summary.state!=PM_AP_IDLE&&probe.summary.state!=PM_AP_FROZEN)probe.trigger.flags|=4u;probe.message=0;probe.active_kind=0;probe.session=probe.frame=0;probe.connection++;if(probe.summary.state!=PM_AP_FROZEN)probe.summary.connections=probe.connection;append(PM_AP_CONNECTION,0,0,0,0,0,probe.connection,0,0,0);}
bool pm_ack_probe_sample_due(void){if(probe.summary.state!=PM_AP_RECORDING)return false;uint64_t now=time_us_64();expire(now);if(probe.summary.state!=PM_AP_RECORDING||now<probe.next_sample)return false;probe.next_sample=now+1000000;return true;}
void pm_ack_probe_sample(uint16_t flags,uint32_t a,uint32_t b,uint32_t c,uint32_t d){append(PM_AP_TCP_SAMPLE,0,probe.message,0,0,flags,a,b,c,d);}
size_t pm_ack_probe_ram_bytes(void){return sizeof probe;}
pm_ack_probe_summary pm_ack_probe_read_summary(void){expire(time_us_64());return probe.summary;}
bool pm_ack_probe_read(unsigned i,pm_ack_probe_record *out){if(i>=probe.summary.count)return false;*out=probe.records[(probe.head+i)%PM_ACK_PROBE_CAP];return true;}
static void response(const char *format,...){
 va_list ap;va_start(ap,format);vsnprintf(probe.reply,sizeof probe.reply,format,ap);va_end(ap);
}
static void status(void){
 response("ap2 status boot=%llx run=%llx state=%u n=%u over=%u gen=%u digest=%x reason=%u\n",(unsigned long long)probe.summary.boot,(unsigned long long)probe.summary.run,probe.summary.state,probe.summary.count,probe.summary.dropped,probe.summary.connections,probe.summary.digest,probe.summary.reason);
}
static bool hex64(const char *s,uint64_t *out){
 if(!*s||strlen(s)>16)return false;
 uint64_t v=0;
 for(;*s;s++){unsigned x;if(*s>='0'&&*s<='9')x=(unsigned)(*s-'0');else if(*s>='a'&&*s<='f')x=(unsigned)(*s-'a'+10);else if(*s>='A'&&*s<='F')x=(unsigned)(*s-'A'+10);else return false;v=(v<<4)|x;}
 *out=v;return true;
}
void pm_ack_probe_command(const char *line){
 expire(time_us_64());
 // Exactly one bounded command at a time; retry STATUS/READ after draining.
 if(probe.reply[0]||probe.reading||probe.pending_length)return;
 char copy[96];if(strlen(line)>=sizeof copy){response("ap2 error=length\n");return;}strcpy(copy,line);
 char *word[6],*save=NULL;unsigned n=0;for(char *p=strtok_r(copy," \r\n",&save);p;p=strtok_r(NULL," \r\n",&save)){if(n==6){response("ap2 error=syntax\n");return;}word[n++]=p;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"CAPS")){response("ap2 caps v=3 hb=1 event=19\n");return;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"TRIGGER")){response("ap2 trigger b=%llx r=%llx mode=%u o=%x g=%x m=%x flags=%x\n",(unsigned long long)probe.summary.boot,(unsigned long long)probe.summary.run,probe.trigger.mode,probe.trigger.origin,probe.trigger.gate,probe.trigger.message,probe.trigger.flags);return;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"SYNTH")){response("ap2 synth boot=%llx run=%llx n=%x job=%llx step=%x\n",(unsigned long long)probe.summary.boot,(unsigned long long)probe.summary.run,probe.sim_count,(unsigned long long)probe.sim_job,probe.sim_step);return;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"QUALITY")){response("ap2 quality boot=%llx run=%llx gate=%llx buttons=%x overlap=%llx\n",(unsigned long long)probe.summary.boot,(unsigned long long)probe.summary.run,(unsigned long long)probe.summary.gate_us,probe.summary.gate_buttons,(unsigned long long)probe.summary.usb_overlap_us);return;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"WINDOW")){response("ap2 window boot=%llx run=%llx trigger=%llx freeze=%llx begin=%x end=%x\n",(unsigned long long)probe.summary.boot,(unsigned long long)probe.summary.run,(unsigned long long)probe.summary.trigger_us,(unsigned long long)probe.summary.freeze_us,probe.summary.arm_buttons,probe.summary.freeze_buttons);return;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"LIVE")){response("ap2 live boot=%llx session=%llx active=%u gen=%x buttons=%x state=%u trigger=%llx begin=%x\n",(unsigned long long)probe.summary.boot,(unsigned long long)probe.live_session,probe.live_active?1u:0u,probe.connection,probe.buttons,probe.summary.state,(unsigned long long)probe.summary.trigger_us,probe.summary.arm_buttons);return;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"ID")){response("ap2 identity boot=%llx device=%s last=%llx\n",(unsigned long long)probe.summary.boot,PM_DEVICE_ID,(unsigned long long)probe.summary.last_run);return;}
 if(n==2&&!strcmp(word[0],"AP2")&&!strcmp(word[1],"STATUS")){status();return;}
 uint64_t boot,run,extra=0;
 if(n<4||strcmp(word[0],"AP2")||!hex64(word[2],&boot)||!hex64(word[3],&run)||!run){response("ap2 error=syntax\n");return;}
 if(boot!=probe.summary.boot){response("ap2 error=boot\n");return;}
 if((!strcmp(word[1],"ARM")||!strcmp(word[1],"ARM_HB"))&&n==4){
  unsigned mode=!strcmp(word[1],"ARM_HB");
  if(probe.summary.state!=PM_AP_IDLE){if(run==probe.summary.run&&mode==probe.trigger.mode)status();else response("ap2 error=retained\n");return;}
  if(run<=probe.summary.last_run){response("ap2 error=old_run\n");return;}
  if(mode&&probe.message==UINT32_MAX){response("ap2 error=ordinal\n");return;}
  memset(&probe.trigger,0,sizeof probe.trigger);probe.trigger.mode=(uint8_t)mode;probe.trigger.origin=probe.message;probe.trigger.gate=probe.message;
  probe.sim_count=probe.sim_step=0;probe.sim_job=0;probe.summary.last_run=run;probe.summary.connections=probe.connection;probe.summary.run=run;probe.summary.arm_buttons=probe.buttons;probe.summary.armed_us=time_us_64();(void)capture_transition(probe.usb_connected?obs_pico_ARM_CONNECTED:obs_pico_ARM_DETACHED);probe.summary.gate_us=probe.usb_connected?0:time_us_64();probe.summary.gate_buttons=probe.buttons;status();return;
 }
 if(!strcmp(word[1],"RELEASE")&&n==5&&probe.summary.state==PM_AP_IDLE&&run==probe.summary.last_run&&hex64(word[4],&extra)&&extra==probe.summary.last_digest){status();return;}
 if(run!=probe.summary.run||probe.summary.state==PM_AP_IDLE){response("ap2 error=run\n");return;}
 if(!strcmp(word[1],"FREEZE")&&n==4){freeze(2,time_us_64());status();return;}
 if(!strcmp(word[1],"READ")&&n==6){
  uint64_t from,count;if(probe.summary.state!=PM_AP_FROZEN){response("ap2 error=not_frozen\n");return;}
  if(!hex64(word[4],&from)||!hex64(word[5],&count)||count<1||count>8||from>probe.summary.count){response("ap2 error=page\n");return;}
  probe.read_from=(uint32_t)from;probe.read_count=(uint32_t)count;if(probe.read_count>probe.summary.count-probe.read_from)probe.read_count=probe.summary.count-probe.read_from;
  probe.read_row=0;probe.reading=true;return;
 }
 if(!strcmp(word[1],"RELEASE")&&n==5&&hex64(word[4],&extra)){
  if(probe.summary.state!=PM_AP_FROZEN||extra!=probe.summary.digest){response("ap2 error=digest\n");return;}
  (void)capture_transition(obs_pico_RELEASE);uint64_t b=probe.summary.boot,last=probe.summary.last_run;uint32_t gen=probe.summary.connections,previous=probe.summary.digest;memset(&probe.summary,0,sizeof probe.summary);probe.summary.boot=b;probe.summary.last_run=last;probe.summary.last_digest=previous;probe.summary.connections=gen;probe.head=0;memset(&probe.trigger,0,sizeof probe.trigger);status();return;
 }
 response("ap2 error=syntax\n");
}
void pm_ack_probe_input(int ch){
 if(ch=='\n'){
  if(probe.input_overflow){if(!probe.reply[0])response("ap2 error=length\n");}
  else {probe.input[probe.input_used]=0;pm_ack_probe_command(probe.input);}
  probe.input_used=0;probe.input_overflow=false;return;
 }
 if(ch=='\r')return;
 if(ch<32||ch>126){probe.input_overflow=true;return;}
 if(probe.input_used+1<sizeof probe.input)probe.input[probe.input_used++]=(char)ch;else probe.input_overflow=true;
}
void pm_ack_probe_poll(bool connected){
 pm_ack_probe_usb_state(connected);
 // USB gates only unstarted runs; it never resets/frees recorded data.
 if(!connected){probe.input_used=0;probe.input_overflow=false;return;}
 uint64_t now=time_us_64();if(now<probe.next_export)return;probe.next_export=now+25000;
 if(!probe.pending_length){
  if(probe.reply[0]){strcpy(probe.pending,probe.reply);probe.reply[0]=0;}
  else if(probe.reading){
   unsigned row=probe.read_row,i=probe.read_from+(row? (row-1)/2:0);pm_ack_probe_record r;
   if(!row)snprintf(probe.pending,sizeof probe.pending,"ap2 page boot=%llx run=%llx from=%x count=%x total=%x digest=%x\n",(unsigned long long)probe.summary.boot,(unsigned long long)probe.summary.run,probe.read_from,probe.read_count,probe.summary.count,probe.summary.digest);
   else if(row<=2*probe.read_count&&pm_ack_probe_read(i,&r)){
    if(row&1)snprintf(probe.pending,sizeof probe.pending,"ap2 i=%x t=%llx e=%x k=%x g=%x m=%x s=%llx f=%llx\n",i,(unsigned long long)r.us,r.event,r.kind,r.connection,r.message,(unsigned long long)r.session,(unsigned long long)r.frame);
    else snprintf(probe.pending,sizeof probe.pending,"ap2 i=%x flags=%x a=%x b=%x c=%x d=%x\n",i,r.flags,r.a,r.b,r.c,r.d);
   }else snprintf(probe.pending,sizeof probe.pending,"ap2 end from=%x count=%x digest=%x\n",probe.read_from,probe.read_count,probe.summary.digest);
  }else return;
  probe.pending_length=strlen(probe.pending);
 }
 if(pm_trace_try_line(probe.pending,probe.pending_length)){
  probe.pending_length=0;
  if(probe.reading){probe.read_row++;if(probe.read_row>2*probe.read_count+1)probe.reading=false;}
 }
}
#endif
