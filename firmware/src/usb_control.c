#include "usb_control.h"
#include "ack_probe.h"
#include "trace.h"
#include "net.h"
#include <string.h>
#include <stdio.h>
#include <stdarg.h>
#include <limits.h>
#if PM_EVENT_STREAM
#include "hardware/sync.h"
/* No formatting, IO or SDK entry while masked. Nested observation helpers
 * restore the masked state; only this outer restore admits IRQ producers. */
static pm_observation_status observation_boundary(bool enabled){
    uint32_t irq=save_and_disable_interrupts();
    if(enabled){
        pm_observation_enable(true);
        pm_observe(PM_OB_OBSERVATION,1,PM_OB_RECEIVED,0,0,0,0,PM_OB_CATALOG,PM_OB_COVERAGE);
    }else{
        pm_observe(PM_OB_OBSERVATION,1,PM_OB_COMPLETED,0,0,0,0,PM_OB_CATALOG,PM_OB_COVERAGE);
        pm_observation_enable(false);
    }
    pm_observation_status status=pm_observation_read();
    restore_interrupts(irq);
    return status;
}
#endif
#define LINE 96u
#define REPLY 144u
#define LEDGER 8u
static struct {
    uint64_t boot,last_command,session;
    unsigned state,used,head,count,ledger_next,reply_drops,buttons;
    uint8_t operations;
    pm_input_handler input;
    uint64_t input_last_id;
    pm_operation input_last_operation;
    pm_input_result input_last_result;
    bool input_have_result;
    uint32_t now;
    bool overflow,connected,turn,radio_pending,radio_roaming,radio_last_roaming;
    unsigned radio_slot;
    pm_radio_change_op radio_op,radio_last_op;
    uint64_t radio_id,radio_last_id;
    char radio_last_reply[REPLY];
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
static bool hex(const char *s,uint64_t *v){
    if(!*s || strlen(s)>16)return false;
    *v=0;
    for(;*s;s++){unsigned c;if(*s>='0'&&*s<='9')c=(unsigned)(*s-'0');else if(*s>='a'&&*s<='f')c=(unsigned)(*s-'a'+10);else if(*s>='A'&&*s<='F')c=(unsigned)(*s-'A'+10);else return false;*v=(*v<<4)|c;}return true;
}
void pm_usb_control_init(uint64_t boot,unsigned buttons){memset(&ctl,0,sizeof ctl);ctl.boot=boot?boot:1;ctl.buttons=buttons;pm_observation_init(ctl.boot);
#if PM_USB_BUTTON_SIM
    pm_sim_init(&ctl.sim,buttons);
#else
    (void)buttons;
#endif
}
void pm_usb_control_set_operations(uint8_t operations,pm_input_handler handler){
    ctl.operations=operations&PM_OPERATION_MASK;ctl.input=handler;
}
static void radio_command(char **w,unsigned n){
    bool roaming=n>=2&&!strcmp(w[1],"ROAMING");
    bool known=n>=2&&(roaming||!strcmp(w[1],"INTERFERENCE"));
    if(n==3&&known&&!strcmp(w[2],"CAPS")){
        response("radio caps b=%llx last_id=%llx last_op=%u\n",(unsigned long long)ctl.boot,
            (unsigned long long)ctl.radio_last_id,(unsigned)ctl.radio_last_op);return;
    }
    uint64_t boot,id=0;pm_radio_change_op op=0;
    if(n>=3&&!strcmp(w[2],roaming?"OFF":"TRY3"))op=PM_RADIO_TRY3;
    if(n>=3&&!strcmp(w[2],"RESTORE"))op=PM_RADIO_RESTORE;
    if(n!=(op?5u:4u)||!known||
        (!op&&strcmp(w[2],"GET"))||!hex(w[3],&boot)||
        (op&&(!hex(w[4],&id)||!id))){response("radio error=syntax\n");return;}
    if(boot!=ctl.boot){response("radio error=boot\n");return;}
    if(!ctl.connected){response("radio error=disconnected\n");return;}
    if(op&&id<=ctl.radio_last_id){
        if(id<ctl.radio_last_id)response("radio error=stale_id\n");
        else if(op!=ctl.radio_last_op||roaming!=ctl.radio_last_roaming)response("radio error=conflict\n");
        else enqueue(ctl.radio_last_reply);
        return;
    }
    if(ctl.radio_pending||ctl.count==2){response("radio error=busy\n");return;}
    ctl.radio_op=op;ctl.radio_id=id;ctl.radio_roaming=roaming;
    ctl.radio_slot=(ctl.head+ctl.count)%2;
    enqueue("");ctl.radio_pending=true;
}
static void radio_change_reply(const char *family,const char *op,const char *status,bool wrote,int write_result,
        pm_radio_read read,bool target_valid,uint32_t target_value,bool base_valid,uint32_t base_value,bool match,const char *cdc){
 char wr[16]="-",target[2]="-",base[2]="-";
 if(wrote)snprintf(wr,sizeof wr,"%d",write_result);
 if(target_valid)target[0]=(char)('0'+target_value);
 if(base_valid)base[0]=(char)('0'+base_value);
 char *reply=ctl.replies[ctl.radio_slot];
 int n=snprintf(reply,REPLY,"radio %s b=%llx id=%llx op=%s s=%s w=%s r=%d x=%08lx t=%s base=%s m=%u cdc_status=%s\n",
    family,(unsigned long long)ctl.boot,(unsigned long long)ctl.radio_id,op,status,wr,read.sdk_result,(unsigned long)read.raw,target,base,match?1u:0u,cdc);
 if(n<0||(size_t)n>=REPLY)snprintf(reply,REPLY,"radio error=response_length\n");
 ctl.radio_last_id=ctl.radio_id;ctl.radio_last_op=ctl.radio_op;ctl.radio_last_roaming=ctl.radio_roaming;
 snprintf(ctl.radio_last_reply,REPLY,"%s",reply);ctl.radio_pending=false;
}
void pm_usb_control_service_radio_change(bool connected,pm_radio_changer changer){
 if(!connected||!ctl.radio_pending||!ctl.radio_op||ctl.radio_roaming)return;
 pm_radio_change result=changer?changer(ctl.radio_op):
    pm_radio_change_rejected(ctl.radio_op,pm_radio_rejected(PM_RADIO_UNAVAILABLE));
 uint32_t target=0,base=0;
 bool have_target=pm_interference_candidate_value(result.target,&target),have_base=pm_interference_candidate_value(result.baseline,&base);
 radio_change_reply("change",ctl.radio_op==PM_RADIO_TRY3?"3":"R",pm_radio_change_status(result),result.wrote,result.write_result,
    result.read,have_target,target,have_base,base,pm_radio_change_match(result),pm_radio_change_cdc(result));
}
void pm_usb_control_service_radio(bool connected,pm_radio_reader reader){
    if(!connected||!ctl.radio_pending||ctl.radio_op||ctl.radio_roaming)return;
    pm_radio_read read=reader?reader():pm_radio_rejected(PM_RADIO_UNAVAILABLE);
    char *reply=ctl.replies[ctl.radio_slot];
    pm_interference_readback readback;uint32_t value;
    if(pm_radio_transport_readback(read,&readback)&&pm_interference_candidate_value(readback.setting,&value))
        snprintf(reply,REPLY,"radio interference b=%llx status=transport_returned rc=0 raw=%08lx candidate=%lu auto_active=%u cdc_status=unverified\n",
            (unsigned long long)ctl.boot,(unsigned long)read.raw,(unsigned long)value,readback.auto_active?1u:0u);
    else if(read.status==PM_RADIO_UNKNOWN)
        snprintf(reply,REPLY,"radio interference b=%llx status=unknown rc=%d raw=%08lx cdc_status=unverified\n",
            (unsigned long long)ctl.boot,read.sdk_result,(unsigned long)read.raw);
    else
        snprintf(reply,REPLY,"radio interference b=%llx status=%s rc=%d cdc_status=%s\n",
            (unsigned long long)ctl.boot,pm_radio_status_name(read.status),read.sdk_result,
            read.status==PM_RADIO_SDK_ERROR?"unverified":"not_read");
    ctl.radio_pending=false;
}
void pm_usb_control_service_roaming(bool connected,pm_radio_reader reader,pm_roaming_changer changer){
 if(!connected||!ctl.radio_pending||!ctl.radio_roaming)return;
 if(ctl.radio_op){
  pm_roaming_op op=ctl.radio_op==PM_RADIO_TRY3?PM_ROAM_OFF:PM_ROAM_RESTORE;
  pm_roaming_change r=changer?changer(op):pm_roaming_rejected(op,pm_radio_rejected(PM_RADIO_UNAVAILABLE));
  uint32_t target=0,base=0;bool have_target=pm_roaming_value(r.target,&target),have_base=pm_roaming_value(r.baseline,&base);
  radio_change_reply("roam",op==PM_ROAM_OFF?"O":"R",pm_roaming_change_status(r),r.wrote,r.write_result,
      r.read,have_target,target,have_base,base,pm_roaming_match(r),pm_roaming_cdc(r));return;
 }
 pm_radio_read r=reader?reader():pm_radio_rejected(PM_RADIO_UNAVAILABLE);pm_roaming_candidate candidate;uint32_t value;
 char *reply=ctl.replies[ctl.radio_slot];int n;
 if(pm_roaming_transport(r,&candidate)&&pm_roaming_value(candidate,&value))
  n=snprintf(reply,REPLY,"radio roaming b=%llx status=transport_returned rc=0 raw=%08lx roam_off_candidate=%lu cdc_status=unverified\n",
      (unsigned long long)ctl.boot,(unsigned long)r.raw,(unsigned long)value);
 else if(r.status==PM_RADIO_UNKNOWN)
  n=snprintf(reply,REPLY,"radio roaming b=%llx status=unknown rc=%d raw=%08lx cdc_status=unverified\n",(unsigned long long)ctl.boot,r.sdk_result,(unsigned long)r.raw);
 else n=snprintf(reply,REPLY,"radio roaming b=%llx status=%s rc=%d cdc_status=%s\n",(unsigned long long)ctl.boot,pm_radio_status_name(r.status),r.sdk_result,r.status==PM_RADIO_SDK_ERROR?"unverified":"not_read");
 if(n<0||(size_t)n>=REPLY)snprintf(reply,REPLY,"radio error=response_length\n");
 ctl.radio_pending=false;
}

static void input_reply(uint64_t id,pm_operation operation,pm_input_result result){
    response("input result b=%llx id=%llx op=%s status=%s\n",(unsigned long long)ctl.boot,
        (unsigned long long)id,pm_operation_name(operation),pm_input_result_name(result));
}
static void input_command(char **w,unsigned n){
    if(n==2&&!strcmp(w[1],"CAPS")){
        response("input caps b=%llx ops=%x keys=%u last_request_id=%llx\n",(unsigned long long)ctl.boot,ctl.operations,ctl.buttons,(unsigned long long)ctl.input_last_id);return;
    }
    uint64_t boot,id;pm_operation operation={0};
    if(n!=5||strcmp(w[1],"DO")||!hex(w[2],&boot)||!hex(w[3],&id)||!id||
       !pm_operation_from_name(w[4],&operation)){response("input error=syntax\n");return;}
    if(boot!=ctl.boot){response("input error=boot\n");return;}
    if(!ctl.connected){response("input error=disconnected\n");return;}
    if(ctl.input_have_result&&id<=ctl.input_last_id){
        if(id<ctl.input_last_id)response("input error=stale_id\n");
        else if(operation.value!=ctl.input_last_operation.value)response("input error=conflict\n");
        else input_reply(id,operation,ctl.input_last_result);
        return;
    }
    if(ctl.count==2){response("input error=busy\n");return;}
    pm_input_result result=ctl.input?ctl.input(operation):PM_INPUT_BUSY;
    ctl.input_last_id=id;ctl.input_last_operation=operation;ctl.input_last_result=result;ctl.input_have_result=true;
    input_reply(id,operation,result);
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
    if(n==4 && !strcmp(w[0],"OBS") && !strcmp(w[1],"METRICS")){
        uint64_t cid,page;
        if(!hex(w[2],&cid) || !cid || !hex(w[3],&page) || page>3){response("obs error=syntax\n");return;}
        pm_observation_metrics m=pm_observation_metrics_read();
        pm_observation_status s=pm_observation_read();uint64_t a,b,c;
        if(page==0){a=m.offered;b=s.dropped;c=m.highwater;}
        else if(page==1){a=m.drain_calls;b=m.write_calls;c=m.bytes;}
        else if(page==2){a=m.partial_writes;b=m.zero_writes;c=m.budget_stops;}
        else {a=m.max_drain_us;b=m.saturated;c=s.queued;}
        response("obs m %llx %llx %llx %llx %llx\n",(unsigned long long)cid,
            (unsigned long long)page,(unsigned long long)a,(unsigned long long)b,(unsigned long long)c);return;
    }
    if(n>=2 && !strcmp(w[0],"EV1")){
        uint64_t cid=0;bool tagged=n==3 && hex(w[2],&cid) && cid;
        if(n!=2 && !tagged){response("ev1 error=syntax\n");return;}
        pm_observation_status s;
        if(!strcmp(w[1],"ON"))s=observation_boundary(true);
        else if(!strcmp(w[1],"OFF"))s=observation_boundary(false);
        else if(strcmp(w[1],"STATUS")){response("ev1 error=syntax\n");return;}
        else s=pm_observation_read();
        if(tagged)response("ev1 s %llx %llx %llx %llx %llx %llx %u %u\n",
            (unsigned long long)cid,(unsigned long long)s.boot,(unsigned long long)s.sequence,
            (unsigned long long)s.dropped,(unsigned long long)s.first_lost,(unsigned long long)s.last_lost,s.enabled,s.saturated);
        else response("ev1 status b=%llx seq=%llx drop=%llx first=%llx last=%llx on=%u sat=%u\n",
            (unsigned long long)s.boot,(unsigned long long)s.sequence,(unsigned long long)s.dropped,
            (unsigned long long)s.first_lost,(unsigned long long)s.last_lost,s.enabled,s.saturated);return;
    }
#endif
    if(n && !strcmp(w[0],"RADIO")){radio_command(w,n);return;}
    if(n && !strcmp(w[0],"INPUT")){input_command(w,n);return;}
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
        if(n==2 && !strcmp(w[1],"CAPS")){response("cy1 caps v=2 rows=%x crc=%x off=%x depth=%x row=%x src=%s\n",PM_CYW_INNER_CAP,PM_CYW_INNER_CRC_CAP,PM_CYW_INNER_OFFSET,PM_CYW_INNER_DEPTH,(unsigned)sizeof(pm_cyw_row),pm_cyw43_inner_translation_unit);return;}
        pm_cyw_status s;pm_net_cyw_status(&s);
        if(n==2 && !strcmp(w[1],"STATUS")){response("cy1 status b=%llx r=%llx n=%x over=%x inc=%x gen=%x active=%u changed=%u h=%x\n",(unsigned long long)s.boot,(unsigned long long)s.run,s.count,s.dropped,s.incomplete,s.generation,s.active,s.changed,s.digest);return;}
        if(n==2 && !strcmp(w[1],"QUALITY")){response("cy1 quality sat=%x depth=%x reject=%x skip=%x crc_calls=%x bytes=%x us=%x max=%x\n",s.saturated,s.depth_lost,s.rejected,s.skipped,s.crc_calls,s.crc_bytes,s.crc_us,s.crc_max_us);return;}
        uint64_t boot,run,index,part;pm_cyw_row r;
        if(n==6 && !strcmp(w[1],"READ") && hex(w[2],&boot) && hex(w[3],&run) && hex(w[4],&index) && hex(w[5],&part) && index<PM_CYW_INNER_CAP && part<7 && pm_net_cyw_read(boot,run,(unsigned)index,&r)){
            if(part==0)response("cy1 row i=%x p=0 t=%llx bus=%llx end=%llx id=%x flags=%x dep=%x nest=%x\n",(unsigned)index,(unsigned long long)r.enter_us,(unsigned long long)r.bus_us,(unsigned long long)r.end_us,r.ticket,r.flags,r.depth,r.nested);
            if(part==1 || part==2){pm_cyw_key *k=part==1?&r.original:&r.bus;response("cy1 row i=%x p=%x s=%x e=%x a=%x ip=%x len=%x et=%x f=%x v=%x\n",(unsigned)index,(unsigned)part,k->seq,k->end,k->ack,k->ipid,k->length,k->type,k->flags,k->valid);}
            if(part==3)response("cy1 row i=%x p=3 c0=%x cr=%x cb=%x b0=%x bb=%x off=%x cov=%x domain=%x\n",(unsigned)index,r.crc_before,r.crc_rx,r.crc_bus,r.bdc_before,r.bdc_bus,PM_CYW_INNER_OFFSET,r.covered,r.domain);
            if(part==4)response("cy1 row i=%x p=4 wait=%x polls=%x d=%x c=%x a=%x bad=%x rxrc=%d n=%x\n",(unsigned)index,r.wait_us,r.polls,r.rx_data,r.rx_control,r.rx_event,r.rx_bad,r.rx_result,r.rx_length);
            if(part==5)response("cy1 row i=%x p=5 calls=%x req=%x errs=%x f0=%x f1=%x c0=%x c1=%x s0=%x s1=%x brc=%ld\n",(unsigned)index,r.io_calls,r.io_requested,r.io_errors,r.flow_before,r.flow_bus,r.credit_before,r.credit_bus,r.sequence_before,r.sequence_bus,(long)r.result);
            if(part==6)response("cy1 row i=%x p=6 valid=%x mac=%02x%02x%02x%02x%02x%02x\n",(unsigned)index,r.validity,r.dest_mac[0],r.dest_mac[1],r.dest_mac[2],r.dest_mac[3],r.dest_mac[4],r.dest_mac[5]);
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
static void send_reply(void){if(ctl.count && !(ctl.radio_pending&&ctl.head==ctl.radio_slot) && pm_trace_try_line(ctl.replies[ctl.head],strlen(ctl.replies[ctl.head]))){ctl.head=(ctl.head+1)%2;ctl.count--;}}
void pm_usb_control_poll(bool connected){
    if(!connected){ctl.used=0;ctl.overflow=false;ctl.head=ctl.count=0;ctl.radio_pending=false;}
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
