#include "usb_control.h"
#if PM_USB_BUTTON_SIM || PM_ACK_PATH_PROBE || PM_EVENT_STREAM
#include "ack_probe.h"
#include "trace.h"
#include "net.h"
#include <string.h>
#include <stdio.h>
#include <stdarg.h>
#include <limits.h>
#define LINE 96u
#define REPLY 144u
#define LEDGER 8u
static struct {
    uint64_t boot,last_command,session;
    unsigned state,used,head,count,ledger_next,reply_drops;
    uint32_t now;
    bool overflow,connected,turn;
    char line[LINE],replies[2][REPLY];
#if PM_USB_BUTTON_SIM
    pm_button_sim sim;
    struct {uint64_t id;char command[LINE],reply[REPLY];} ledger[LEDGER];
#endif
} ctl;
static void enqueue(const char *text){
    if(ctl.count==2){if(ctl.reply_drops<UINT_MAX)ctl.reply_drops++;return;}
    snprintf(ctl.replies[(ctl.head+ctl.count)%2],REPLY,"%s",text);ctl.count++;
}
static void response(const char *format,...){
    char line[REPLY];va_list args;va_start(args,format);int n=vsnprintf(line,sizeof line,format,args);va_end(args);
    if(n<0 || (size_t)n>=sizeof line){enqueue("usb error=response_length\n");return;}enqueue(line);
}
#if PM_USB_BUTTON_SIM || PM_TX_LINK_PROBE || PM_CYW43_INNER_PROBE || PM_EVENT_STREAM
static bool hex(const char *s,uint64_t *v){
    if(!*s || strlen(s)>16)return false;
    *v=0;
    for(;*s;s++){unsigned c;if(*s>='0'&&*s<='9')c=(unsigned)(*s-'0');else if(*s>='a'&&*s<='f')c=(unsigned)(*s-'a'+10);else if(*s>='A'&&*s<='F')c=(unsigned)(*s-'A'+10);else return false;*v=(*v<<4)|c;}return true;
}
#endif
void pm_usb_control_init(uint64_t boot,unsigned buttons){memset(&ctl,0,sizeof ctl);ctl.boot=boot?boot:1;pm_observation_init(ctl.boot);
#if PM_USB_BUTTON_SIM
    pm_sim_init(&ctl.sim,buttons);
#else
    (void)buttons;
#endif
}
const pm_button_sim *pm_usb_sim_status(void){
#if PM_USB_BUTTON_SIM
    return &ctl.sim;
#else
    return NULL;
#endif
}
#if PM_USB_BUTTON_SIM
static void run_identity(uint64_t *boot,uint64_t *run){
    uint64_t deadline=0,freeze=0;unsigned state=0;uint32_t generation=0;
    *boot=ctl.boot;*run=0;
    (void)pm_ack_probe_capture(boot,run,&deadline,&state,&freeze,&generation);
}
static void guard_run(void){
    if(ctl.sim.phase!=PM_SIM_STAGING && ctl.sim.phase!=PM_SIM_WAIT_CLOSE && ctl.sim.phase!=PM_SIM_RUNNING)return;
    uint64_t boot,run;run_identity(&boot,&run);
    if(boot!=ctl.sim.bound_boot || run!=ctl.sim.bound_run)pm_sim_cancel(&ctl.sim,PM_SIM_CANCEL_RUN,ctl.now);
}
static unsigned mask(unsigned field){unsigned value=0;for(unsigned i=0;i<ctl.sim.count;i++){pm_sim_step *s=&ctl.sim.steps[i];bool set=field==0?s->accepted:field==1?s->dispatched:field==2?s->released:s->cancelled;if(set)value|=1u<<i;}return value;}
static void sim_status(void){response("sim status b=%llx j=%llx phase=%u n=%u a=%x d=%x r=%x c=%x why=%u drops=%x\n",(unsigned long long)ctl.boot,(unsigned long long)ctl.sim.id,ctl.sim.phase,ctl.sim.count,mask(0),mask(1),mask(2),mask(3),ctl.sim.reason,ctl.reply_drops);}
static const char *result(pm_sim_result r){return r==PM_SIM_OK?"ok":r==PM_SIM_BUSY?"busy":r==PM_SIM_OLD_JOB?"old_job":r==PM_SIM_CONFLICT?"conflict":r==PM_SIM_UNSUPPORTED?"unsupported":"invalid";}
static void sim_command(char **w,unsigned n,const char *canonical){
    guard_run();
    if(n==2 && !strcmp(w[1],"CAPS")){
        response("sim caps b=%llx keys=%u steps=%u job_ms=%x hold_ms=%x gate_ms=%x s=%llx state=%u\n",(unsigned long long)ctl.boot,ctl.sim.buttons,PM_SIM_STEPS,PM_SIM_JOB_MAX_MS,PM_SIM_HOLD_MAX_MS,PM_SIM_CLOSE_GUARD_MS,(unsigned long long)ctl.session,ctl.state);return;
    }
    uint64_t boot,cid,job=0,v[4]={0};
    if(n<3 || !hex(w[2],&boot) || boot!=ctl.boot){response("sim error=boot\n");return;}
    if(n==3 && !strcmp(w[1],"STATUS")){sim_status();return;}
    if(n==3 && !strcmp(w[1],"BIND")){
        response("sim bind b=%llx j=%llx rb=%llx r=%llx s=%llx state=%u\n",(unsigned long long)ctl.boot,(unsigned long long)ctl.sim.id,(unsigned long long)ctl.sim.bound_boot,(unsigned long long)ctl.sim.bound_run,(unsigned long long)ctl.sim.bound_session,ctl.sim.expected_state);return;
    }
    if(n==4 && !strcmp(w[1],"ACK") && hex(w[3],&cid) && cid){
        for(unsigned i=0;i<LEDGER;i++)if(ctl.ledger[i].id==cid){enqueue(ctl.ledger[i].reply);return;}
        response("sim error=unknown_command\n");return;
    }
    if(n==5 && !strcmp(w[1],"GET") && hex(w[3],&job) && hex(w[4],&v[0]) && job==ctl.sim.id && v[0]<ctl.sim.count){
        pm_sim_step *s=&ctl.sim.steps[v[0]];response("sim step j=%llx i=%x a=%u d=%u r=%u c=%u key=%u logical=%d dt=%x rt=%x\n",(unsigned long long)job,(unsigned)v[0],s->accepted,s->dispatched,s->released,s->cancelled,s->key,s->logical,s->dispatched_ms,s->released_ms);return;
    }
    if(n<4 || !hex(w[3],&cid) || !cid){response("sim error=command_id\n");return;}
    for(unsigned i=0;i<LEDGER;i++)if(ctl.ledger[i].id==cid){
        if(strcmp(ctl.ledger[i].command,canonical))response("sim error=conflict\n");else enqueue(ctl.ledger[i].reply);return;
    }
    if(cid<=ctl.last_command){response("sim error=old_command\n");return;}
    bool stop=!strcmp(w[1],"STOP") || !strcmp(w[1],"CANCEL") || !strcmp(w[1],"RELEASE");
    pm_sim_result r=PM_SIM_INVALID;const char *label=NULL;
    if(!stop && (ctl.count || pm_ack_probe_usb_busy()))label="busy";
    else if(n==4 && !strcmp(w[1],"STOP")){pm_sim_cancel(&ctl.sim,PM_SIM_CANCEL_COMMAND,ctl.now);r=PM_SIM_OK;job=ctl.sim.id;}
    else if(n>=5 && hex(w[4],&job)){
        bool numbers=true;for(unsigned i=5;i<n;i++)if(i-5>=4 || !hex(w[i],&v[i-5]))numbers=false;
        if(numbers){
            if(n==8 && !strcmp(w[1],"BEGIN") && v[0]<=PM_SIM_STEPS && v[2]<=7 && v[1]==ctl.session && v[2]==ctl.state){r=pm_sim_begin(&ctl.sim,job,(unsigned)v[0],v[1],(unsigned)v[2],ctl.now);if(r==PM_SIM_OK)label="staging";}
            else if(n==9 && !strcmp(w[1],"STEP") && v[0]<PM_SIM_STEPS && v[1]<=UINT32_MAX && v[2]<4 && v[3]<=UINT32_MAX){r=pm_sim_step_set(&ctl.sim,job,(unsigned)v[0],(uint32_t)v[1],(unsigned)v[2],(uint32_t)v[3]);if(r==PM_SIM_OK)label="staged";}
            else if(n==5 && !strcmp(w[1],"COMMIT")){r=pm_sim_commit(&ctl.sim,job,ctl.now);if(r==PM_SIM_OK)label="accepted";}
            else if(n==7 && !strcmp(w[1],"PRESS") && v[0]<4 && v[1]<=UINT32_MAX){r=pm_sim_press(&ctl.sim,job,(unsigned)v[0],(uint32_t)v[1],ctl.session,ctl.state,ctl.now);if(r==PM_SIM_OK)label="accepted";}
            else if(n==5 && !strcmp(w[1],"RELEASE")){r=pm_sim_release(&ctl.sim,job,ctl.now);}
            else if(n==5 && !strcmp(w[1],"CANCEL") && job==ctl.sim.id){pm_sim_cancel(&ctl.sim,PM_SIM_CANCEL_COMMAND,ctl.now);r=PM_SIM_OK;}
        }
    }
    if(r==PM_SIM_OK && (!strcmp(w[1],"BEGIN") || !strcmp(w[1],"PRESS"))){run_identity(&ctl.sim.bound_boot,&ctl.sim.bound_run);ctl.sim.bound_session=ctl.session;}
    if(!label)label=result(r);
    unsigned slot=ctl.ledger_next++%LEDGER;ctl.ledger[slot].id=cid;ctl.last_command=cid;
    snprintf(ctl.ledger[slot].command,LINE,"%s",canonical);
    snprintf(ctl.ledger[slot].reply,REPLY,"sim ack b=%llx cid=%llx job=%llx result=%s\n",(unsigned long long)ctl.boot,(unsigned long long)cid,(unsigned long long)job,label);
    enqueue(ctl.ledger[slot].reply);
}
#endif
static void command(const char *line){
    char copy[LINE],canonical[LINE];char *w[10],*save=NULL;unsigned n=0;size_t used=0;
    snprintf(copy,sizeof copy,"%s",line);
    for(char *v=strtok_r(copy," \t",&save);v;v=strtok_r(NULL," \t",&save)){
        if(n==10){response("usb error=syntax\n");return;}w[n++]=v;size_t length=strlen(v);
        if(used+length+(used?1:0)>=LINE){response("usb error=length\n");return;}
        if(used)canonical[used++]=' ';
        memcpy(canonical+used,v,length);used+=length;
    }
    canonical[used]=0;
#if PM_EVENT_STREAM
    if(n>=2 && !strcmp(w[0],"EV1")){
        uint64_t cid=0;bool tagged=n==3 && hex(w[2],&cid) && cid;
        if(n!=2 && !tagged){response("ev1 error=syntax\n");return;}
        if(!strcmp(w[1],"ON")){
            pm_observation_enable(true);
            pm_observe(PM_OB_OBSERVATION,1,PM_OB_RECEIVED,0,0,0,0,PM_OB_CATALOG,PM_OB_COVERAGE);
        }else if(!strcmp(w[1],"OFF")){
            pm_observe(PM_OB_OBSERVATION,1,PM_OB_COMPLETED,0,0,0,0,PM_OB_CATALOG,PM_OB_COVERAGE);
            pm_observation_enable(false);
        }
        else if(strcmp(w[1],"STATUS")){response("ev1 error=syntax\n");return;}
        pm_observation_status s=pm_observation_read();
        if(tagged)response("ev1 s %llx %llx %llx %llx %llx %llx %u %u\n",
            (unsigned long long)cid,(unsigned long long)s.boot,(unsigned long long)s.sequence,
            (unsigned long long)s.dropped,(unsigned long long)s.first_lost,(unsigned long long)s.last_lost,s.enabled,s.saturated);
        else response("ev1 status b=%llx seq=%llx drop=%llx first=%llx last=%llx on=%u sat=%u\n",
            (unsigned long long)s.boot,(unsigned long long)s.sequence,(unsigned long long)s.dropped,
            (unsigned long long)s.first_lost,(unsigned long long)s.last_lost,s.enabled,s.saturated);return;
    }
#endif
    if(n && !strcmp(w[0],"AP2")){
        if(ctl.count){response("usb error=busy\n");return;}pm_ack_probe_command(canonical);
#if PM_USB_BUTTON_SIM
        guard_run();
#endif
        return;
    }
#if PM_USB_BUTTON_SIM
    if(n && !strcmp(w[0],"SIM")){sim_command(w,n,canonical);return;}
#endif
#if PM_TX_LINK_PROBE
    if(n>=2 && !strcmp(w[0],"TX1")){
        pm_tx_status s;pm_net_tx_status(&s);
        if(n==2 && !strcmp(w[1],"STATUS")){response("tx1 status b=%llx r=%llx n=%x over=%x gen=%x active=%u ok=%u changed=%u bad=%x h=%x\n",(unsigned long long)s.boot,(unsigned long long)s.run,s.count,s.dropped,s.generation,s.active,s.attached,s.generation_changed,s.malformed,s.digest);return;}
        uint64_t boot,run,index;pm_tx_row row;
        if(n==5 && !strcmp(w[1],"READ") && hex(w[2],&boot) && hex(w[3],&run) && hex(w[4],&index) && index<PM_TX_LINK_CAP && pm_net_tx_read(boot,run,(unsigned)index,&row)){
            response("tx1 row i=%x t=%llx s=%x e=%x a=%x len=%x ip=%x dt=%x rc=%d nr=%x f=%x ref=%x\n",(unsigned)index,(unsigned long long)row.time_us,row.seq,row.end,row.ack,row.length,row.ipid,row.duration_us,row.result,row.nrtx,row.tcp_flags,row.pbuf_ref);return;
        }
        response("tx1 error=identity_or_state\n");return;
    }
#endif
#if PM_CYW43_INNER_PROBE
    if(n>=2 && !strcmp(w[0],"CY1")){
        if(n==2 && !strcmp(w[1],"CAPS")){response("cy1 caps v=1 rows=%x crc=%x off=%x depth=%x row=%x src=%s\n",PM_CYW_INNER_CAP,PM_CYW_INNER_CRC_CAP,PM_CYW_INNER_OFFSET,PM_CYW_INNER_DEPTH,(unsigned)sizeof(pm_cyw_row),pm_cyw43_inner_translation_unit);return;}
        pm_cyw_status s;pm_net_cyw_status(&s);
        if(n==2 && !strcmp(w[1],"STATUS")){response("cy1 status b=%llx r=%llx n=%x over=%x inc=%x gen=%x active=%u changed=%u h=%x\n",(unsigned long long)s.boot,(unsigned long long)s.run,s.count,s.dropped,s.incomplete,s.generation,s.active,s.changed,s.digest);return;}
        if(n==2 && !strcmp(w[1],"QUALITY")){response("cy1 quality sat=%x depth=%x reject=%x skip=%x crc_calls=%x bytes=%x us=%x max=%x\n",s.saturated,s.depth_lost,s.rejected,s.skipped,s.crc_calls,s.crc_bytes,s.crc_us,s.crc_max_us);return;}
        uint64_t boot,run,index,part;pm_cyw_row r;
        if(n==6 && !strcmp(w[1],"READ") && hex(w[2],&boot) && hex(w[3],&run) && hex(w[4],&index) && hex(w[5],&part) && index<PM_CYW_INNER_CAP && part<6 && pm_net_cyw_read(boot,run,(unsigned)index,&r)){
            if(part==0)response("cy1 row i=%x p=0 t=%llx bus=%llx end=%llx id=%x flags=%x dep=%x nest=%x\n",(unsigned)index,(unsigned long long)r.enter_us,(unsigned long long)r.bus_us,(unsigned long long)r.end_us,r.ticket,r.flags,r.depth,r.nested);
            if(part==1 || part==2){pm_cyw_key *k=part==1?&r.original:&r.bus;response("cy1 row i=%x p=%x s=%x e=%x a=%x ip=%x len=%x et=%x f=%x v=%x\n",(unsigned)index,(unsigned)part,k->seq,k->end,k->ack,k->ipid,k->length,k->type,k->flags,k->valid);}
            if(part==3)response("cy1 row i=%x p=3 c0=%x cr=%x cb=%x b0=%x bb=%x off=%x cov=%x domain=%x\n",(unsigned)index,r.crc_before,r.crc_rx,r.crc_bus,r.bdc_before,r.bdc_bus,PM_CYW_INNER_OFFSET,r.covered,r.domain);
            if(part==4)response("cy1 row i=%x p=4 wait=%x polls=%x d=%x c=%x a=%x bad=%x rxrc=%d n=%x\n",(unsigned)index,r.wait_us,r.polls,r.rx_data,r.rx_control,r.rx_event,r.rx_bad,r.rx_result,r.rx_length);
            if(part==5)response("cy1 row i=%x p=5 calls=%x req=%x errs=%x f0=%x f1=%x c0=%x c1=%x s0=%x s1=%x brc=%ld\n",(unsigned)index,r.io_calls,r.io_requested,r.io_errors,r.flow_before,r.flow_bus,r.credit_before,r.credit_bus,r.sequence_before,r.sequence_bus,(long)r.result);
            return;
        }
        response("cy1 error=identity_or_state\n");return;
    }
#endif
    response("usb error=syntax\n");
}
void pm_usb_control_input(int ch){
    if(ch=='\r')return;
    if(ch=='\n'){
        if(ctl.overflow)response("usb error=length\n");else{ctl.line[ctl.used]=0;command(ctl.line);}
        ctl.used=0;ctl.overflow=false;return;
    }
    if(ch<32 || ch>126){ctl.overflow=true;return;}
    if(ctl.used+1<LINE)ctl.line[ctl.used++]=(char)ch;else ctl.overflow=true;
}
static void send_reply(void){if(ctl.count && pm_trace_try_line(ctl.replies[ctl.head],strlen(ctl.replies[ctl.head]))){ctl.head=(ctl.head+1)%2;ctl.count--;}}
void pm_usb_control_poll(bool connected){
    if(!connected){ctl.used=0;ctl.overflow=false;ctl.head=ctl.count=0;}
    ctl.turn=!ctl.turn;
    if(ctl.turn)send_reply();
    pm_ack_probe_poll(connected);
    if(!ctl.turn)send_reply();
}
pm_sim_dispatch pm_usb_control_tick(bool connected,unsigned physical,bool allowed,uint64_t session,unsigned state,bool standby,uint32_t long_ms,uint32_t now){
    ctl.connected=connected;ctl.now=now;ctl.session=session;ctl.state=state;
#if PM_USB_BUTTON_SIM
    guard_run();
    return pm_sim_tick(&ctl.sim,connected,physical,allowed,session,state,standby,long_ms,now);
#else
    (void)physical;(void)allowed;(void)standby;(void)long_ms;return (pm_sim_dispatch){0};
#endif
}
void pm_usb_control_dispatched(pm_sim_dispatch event,uint64_t session,unsigned state,uint32_t now){
#if PM_USB_BUTTON_SIM
    pm_sim_dispatched(&ctl.sim,event.step,event.logical,now,session,state);
    ctl.session=session;ctl.state=state;
#else
    (void)event;(void)session;(void)state;(void)now;
#endif
}
#endif
