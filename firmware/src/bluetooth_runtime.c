/* BT-AUDIO-20261107: bounded shared-runtime adapter. Hardware validation remains required. */
#include "bluetooth_runtime.h"
#include "bluetooth_connection_policy.h"
#include "bluetooth_recovery.h"
#include "bluetooth_event_queue.h"
#include "bluetooth_packet_policy.h"
#include "bluetooth_rebuffer.h"
#include "btstack.h"
#include "classic/btstack_sbc_bluedroid.h"
static pm_bt_event_queue events;
static uint64_t generation, stop_generation;
static bool stopping;
static bool init_ok, initialized, ready, wanted, scanning;
static bd_addr_t peer;
static uint16_t cid;
static uint8_t seid;
static uint16_t remote_cid;
static uint16_t observed_packet_handle,observed_packet_types,observed_mode_handle,observed_mode_interval;
static bool observed_packet_valid,observed_mode_valid;
#ifndef PM_BT_INCOMING_MASTER_POLICY
#define PM_BT_INCOMING_MASTER_POLICY 0
#endif
static uint32_t rate, timestamp, deadline;
static uint64_t accepted_frames, consumed_frames, samples_sent;
static uint64_t codec_epoch;
static pm_bt_sbc_config codec_config;
static bool streaming, pending, input_enabled;
static pm_bluetooth_audio audio;
static btstack_timer_source_t timer;
static btstack_sbc_encoder_bluedroid_t encoder_state;
static const btstack_sbc_encoder_t *encoder;
static uint8_t encoded[1000];
static unsigned prepared_count,prepared_bytes;
static void prepare_audio(void);
static uint32_t can_requested_ms,timer_due_ms,last_packet_end_ms;
static pm_bt_rebuffer rebuffer;
static bool timer_scheduled;
static pm_bt_audio_diagnostics diagnostics;
void pm_bt_runtime_audio_diagnostics(pm_bt_audio_diagnostics *out){if(out){*out=diagnostics;out->rebuffering=rebuffer.buffering;}}
static bool clock_started,pcm_eof;
static uint64_t pcm_end,pcm_first;
static void tick(btstack_timer_source_t *t);
static uint8_t caps[]={0x33,0x15,2,53}, configuration[4];
static uint8_t source_record[150], target_record[200];
static btstack_packet_callback_registration_t registration;
static void emit(pm_bt_event e);
static btstack_timer_source_t connect_timer;
static uint64_t retry_generation;
static pm_bt_recovery recovery;
static btstack_timer_source_t recovery_timer;
static bool recovery_releasing(void){return recovery.phase==PM_BR_RELEASE||recovery.phase==PM_BR_COOLDOWN;}
static void recovery_cancel(void){btstack_run_loop_remove_timer(&recovery_timer);pm_bt_recovery_cancel(&recovery);}
static void recovery_poll(btstack_timer_source_t *t);
static void recovery_fail(uint16_t status);
static bool retry_pending;
static void retry_cancel(void){btstack_run_loop_remove_timer(&connect_timer);retry_pending=false;}
static void retry_connect(btstack_timer_source_t *t);
static void retry_schedule(void){
 if(!wanted||stopping||retry_pending||recovery_releasing())return;
 if(!pm_bt_recovery_reserve(&recovery)){recovery_fail(ERROR_CODE_ACL_CONNECTION_ALREADY_EXISTS);return;}
 recovery.observed_cid=0;
 retry_generation=generation;retry_pending=true;
 btstack_run_loop_set_timer(&connect_timer,500);btstack_run_loop_add_timer(&connect_timer);
}

static void emit(pm_bt_event e){
 if(!e.generation)e.generation=generation;
 pm_bt_event_enqueue(&events,e);
}
static void retry_connect(btstack_timer_source_t *t){
 (void)t;retry_pending=false;
 if(!wanted||stopping||retry_generation!=generation)return;
 if((uint32_t)(btstack_run_loop_get_time_ms()-recovery.started)>=25000){recovery.reason=PM_BR_REASON_GLOBAL;recovery_fail(0xff10);return;}
 /* Lookup is scoped to the explicitly selected peer, after SDK cleanup. */
 avdtp_connection_t *existing=avdtp_get_connection_for_bd_addr(peer);
 if(existing)cid=existing->avdtp_cid;
 uint16_t next=cid;
 uint8_t status=a2dp_source_establish_stream(peer,&next);
 if(!status){cid=next;return;}
 if(status==ERROR_CODE_ACL_CONNECTION_ALREADY_EXISTS||
    (existing&&status==ERROR_CODE_COMMAND_DISALLOWED)){retry_schedule();return;}
 recovery_fail(status);
}
bool pm_bt_runtime_next(pm_bt_event *e){return pm_bt_event_dequeue(&events,e);}
uint32_t pm_bt_runtime_dropped(void){return events.dropped;}
static uint32_t input_epoch;
#include "bluetooth_audio_runtime.inc"
#include "bluetooth_recovery_runtime.inc"
bool pm_bt_runtime_ready(void){return ready&&!stopping;}
static void stopped_if_idle(void){
 if(stopping&&!cid&&!remote_cid&&!scanning){
  stopping=false;emit((pm_bt_event){.kind=PM_BT_STOPPED,.generation=stop_generation});
 }
}
static void hci_handler(uint8_t type,uint16_t channel,uint8_t *p,uint16_t size){
 (void)channel;(void)size;if(type!=HCI_EVENT_PACKET)return;
 bd_addr_t address;
 switch(hci_event_packet_get_type(p)){
 case HCI_EVENT_CONNECTION_PACKET_TYPE_CHANGED:
  if(hci_event_connection_packet_type_changed_get_status(p)==0){
   observed_packet_handle=hci_event_connection_packet_type_changed_get_handle(p);
   observed_packet_types=hci_event_connection_packet_type_changed_get_packet_types(p);observed_packet_valid=true;
  }break;
 case HCI_EVENT_MODE_CHANGE:
  if(hci_event_mode_change_get_status(p)==0){
   observed_mode_handle=hci_event_mode_change_get_handle(p);
   observed_mode_interval=hci_event_mode_change_get_interval(p);observed_mode_valid=true;
  }break;
 case HCI_EVENT_DISCONNECTION_COMPLETE:
  if(hci_event_disconnection_complete_get_connection_handle(p)==observed_packet_handle)observed_packet_valid=false;
  if(hci_event_disconnection_complete_get_connection_handle(p)==observed_mode_handle)observed_mode_valid=false;
  break;
 case BTSTACK_EVENT_STATE:
  ready=btstack_event_state_get_state(p)==HCI_STATE_WORKING;
  if(ready)emit((pm_bt_event){.kind=PM_BT_READY});break;
 case GAP_EVENT_INQUIRY_RESULT:{
  if(!scanning)return;
  pm_bt_event e={.kind=PM_BT_FOUND};gap_event_inquiry_result_get_bd_addr(p,e.address);
  e.device_class=gap_event_inquiry_result_get_class_of_device(p);
  if(gap_event_inquiry_result_get_name_available(p)){unsigned n=gap_event_inquiry_result_get_name_len(p);if(n>63)n=63;memcpy(e.name,gap_event_inquiry_result_get_name(p),n);}
  emit(e);break;}
 case GAP_EVENT_INQUIRY_COMPLETE:
  if(scanning){scanning=false;if(!stopping)emit((pm_bt_event){.kind=PM_BT_SCAN_DONE});}stopped_if_idle();break;
 case HCI_EVENT_PIN_CODE_REQUEST:
  hci_event_pin_code_request_get_bd_addr(p,address);gap_pin_code_negative(address);break;
 case HCI_EVENT_USER_CONFIRMATION_REQUEST:
  hci_event_user_confirmation_request_get_bd_addr(p,address);
  if(wanted&&!memcmp(peer,address,6))gap_ssp_confirmation_response(address);
  else gap_ssp_confirmation_negative(address);break;
 }
}
#include "bluetooth_stream_established.inc"
#include "bluetooth_signaling_released.inc"
static void source_handler(uint8_t type,uint16_t channel,uint8_t *p,uint16_t size){
 (void)channel;(void)size;if(type!=HCI_EVENT_PACKET||hci_event_packet_get_type(p)!=HCI_EVENT_A2DP_META)return;
 switch(p[2]){
 case A2DP_SUBEVENT_SIGNALING_CONNECTION_ESTABLISHED:{
  uint16_t c=a2dp_subevent_signaling_connection_established_get_a2dp_cid(p);
  uint8_t status=a2dp_subevent_signaling_connection_established_get_status(p);
  bd_addr_t incoming;a2dp_subevent_signaling_connection_established_get_bd_addr(p,incoming);
  if(!status){
   if(!pm_bt_accept_peer(wanted,stopping,peer,incoming)||recovery_releasing()){a2dp_source_disconnect(c);return;}
   /* The selected speaker may win simultaneous establishment. Adopt its CID;
    * callbacks for the losing outgoing CID can no longer clear this owner. */
   uint16_t previous=cid;
   if(previous!=c){media_stop();encoder=NULL;rate=0;codec_config=(pm_bt_sbc_config){0};}
   cid=c;retry_cancel();
   emit((pm_bt_event){.kind=PM_BT_LINK_DIAGNOSTIC,.detail1=previous,.detail2=c,.detail3=1});
   return;
  }
  if(!pm_bt_owns_failure(cid,c))return;
  if(!wanted||stopping){cid=0;stopped_if_idle();return;}
  cid=0;
  if(status==ERROR_CODE_ACL_CONNECTION_ALREADY_EXISTS){
   emit((pm_bt_event){.kind=PM_BT_LINK_DIAGNOSTIC,.status=status,.detail1=c,.detail3=2});
   retry_schedule();return;
  }
  retry_cancel();recovery_fail(status);
  break;}
 case A2DP_SUBEVENT_SIGNALING_MEDIA_CODEC_SBC_CONFIGURATION:{
  if(!wanted||recovery_releasing()||a2dp_subevent_signaling_media_codec_sbc_configuration_get_a2dp_cid(p)!=cid)return;
  /* Every accepted reconfiguration invalidates the audio owner and prepared RTP. */
  media_stop();codec_config=(pm_bt_sbc_config){0};
  if(codec_epoch==UINT64_MAX){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff03});pm_bt_runtime_stop(generation);return;}
  codec_epoch++;
  rate=a2dp_subevent_signaling_media_codec_sbc_configuration_get_sampling_frequency(p);
  unsigned channels=a2dp_subevent_signaling_media_codec_sbc_configuration_get_num_channels(p);
  unsigned mode=a2dp_subevent_signaling_media_codec_sbc_configuration_get_channel_mode(p);
  if(a2dp_subevent_signaling_media_codec_sbc_configuration_get_block_length(p)!=16||
     a2dp_subevent_signaling_media_codec_sbc_configuration_get_subbands(p)!=8||
     a2dp_subevent_signaling_media_codec_sbc_configuration_get_max_bitpool_value(p)>53||
     a2dp_subevent_signaling_media_codec_sbc_configuration_get_max_bitpool_value(p)<2||channels!=2||(rate!=44100&&rate!=48000)||(mode!=AVDTP_CHANNEL_MODE_STEREO&&mode!=AVDTP_CHANNEL_MODE_JOINT_STEREO)){
   emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff03});pm_bt_runtime_stop(generation);return;
  }
  unsigned bitpool=pm_bt_choose_bitpool(
   a2dp_subevent_signaling_media_codec_sbc_configuration_get_min_bitpool_value(p),
   a2dp_subevent_signaling_media_codec_sbc_configuration_get_max_bitpool_value(p));
  if(!bitpool){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff03});pm_bt_runtime_stop(generation);return;}
  unsigned allocation=a2dp_subevent_signaling_media_codec_sbc_configuration_get_allocation_method(p);
  if(allocation!=AVDTP_SBC_ALLOCATION_METHOD_LOUDNESS&&allocation!=AVDTP_SBC_ALLOCATION_METHOD_SNR){
   emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff03});pm_bt_runtime_stop(generation);return;
  }
  codec_config=(pm_bt_sbc_config){.rate=rate,.channels=2,.blocks=16,.subbands=8,
   .channel_mode=mode==AVDTP_CHANNEL_MODE_STEREO?2:3,
   .allocation=allocation==AVDTP_SBC_ALLOCATION_METHOD_LOUDNESS?0:1,.bitpool=(uint8_t)bitpool};
  encoder=btstack_sbc_encoder_bluedroid_init_instance(&encoder_state);
  encoder->configure(&encoder_state,SBC_MODE_STANDARD,
   a2dp_subevent_signaling_media_codec_sbc_configuration_get_block_length(p),
   a2dp_subevent_signaling_media_codec_sbc_configuration_get_subbands(p),
   allocation==AVDTP_SBC_ALLOCATION_METHOD_LOUDNESS?SBC_ALLOCATION_METHOD_LOUDNESS:SBC_ALLOCATION_METHOD_SNR,rate,
   bitpool,
   mode==AVDTP_CHANNEL_MODE_STEREO?SBC_CHANNEL_MODE_STEREO:SBC_CHANNEL_MODE_JOINT_STEREO);
  emit((pm_bt_event){.kind=PM_BT_CODEC,.rate=rate});break;}
 case A2DP_SUBEVENT_STREAM_STARTED:
  if(!wanted||a2dp_subevent_stream_started_get_a2dp_cid(p)!=cid)return;
  if(!audio.active){a2dp_source_pause_stream(cid,seid);return;}
  if(streaming)return;
  streaming=true;clock_started=false;timestamp=0;samples_sent=0;deadline=btstack_run_loop_get_time_ms();tick(&timer);break;
 case A2DP_SUBEVENT_STREAMING_CAN_SEND_MEDIA_PACKET_NOW:
  if(a2dp_subevent_streaming_can_send_media_packet_now_get_a2dp_cid(p)==cid)send_audio();break;
 case A2DP_SUBEVENT_STREAM_SUSPENDED:
  if(little_endian_read_16(p,3)==cid&&streaming)media_stop();break;
 case A2DP_SUBEVENT_STREAM_RELEASED:
  if(little_endian_read_16(p,3)==cid)media_stop();break;

 case A2DP_SUBEVENT_STREAM_ESTABLISHED:
  handle_stream_established(a2dp_subevent_stream_established_get_a2dp_cid(p),a2dp_subevent_stream_established_get_status(p));break;
 case A2DP_SUBEVENT_SIGNALING_CONNECTION_RELEASED:
  handle_signaling_released(a2dp_subevent_signaling_connection_released_get_a2dp_cid(p));break;
 }
}
#include "bluetooth_remote_input.inc"
static void remote_handler(uint8_t type,uint16_t channel,uint8_t *p,uint16_t size){
 (void)channel;(void)size;if(type!=HCI_EVENT_PACKET||hci_event_packet_get_type(p)!=HCI_EVENT_AVRCP_META)return;
 if(p[2]==AVRCP_SUBEVENT_CONNECTION_ESTABLISHED){
  bd_addr_t addr;avrcp_subevent_connection_established_get_bd_addr(p,addr);
  uint16_t c=avrcp_subevent_connection_established_get_avrcp_cid(p);
  if(avrcp_subevent_connection_established_get_status(p)){if(c==remote_cid)remote_cid=0;stopped_if_idle();return;}
  if(!wanted||recovery_releasing()||memcmp(addr,peer,6)){avrcp_disconnect(c);return;}remote_cid=c;return;
 }
 if(p[2]==AVRCP_SUBEVENT_CONNECTION_RELEASED){if(avrcp_subevent_connection_released_get_avrcp_cid(p)==remote_cid){remote_cid=0;input_epoch++;}stopped_if_idle();return;}
 if(p[2]!=AVRCP_SUBEVENT_OPERATION)return;
 uint16_t c=avrcp_subevent_operation_get_avrcp_cid(p);
 uint8_t op=avrcp_subevent_operation_get_operation_id(p);
 bool down=avrcp_subevent_operation_get_button_pressed(p)!=0;
 handle_remote_input(c,op,down);
}
bool pm_bt_runtime_init(void){
 if(initialized)return init_ok;
 initialized=true;
 /* Match SDK A2DP source demo: request local master on incoming reconnect.
  * This is a requested policy, not evidence the peer/controller granted it. */
 hci_set_master_slave_policy(PM_BT_INCOMING_MASTER_POLICY);
 l2cap_init();sdp_init();a2dp_source_init();a2dp_source_register_packet_handler(source_handler);
 avdtp_stream_endpoint_t *ep=a2dp_source_create_stream_endpoint(AVDTP_AUDIO,AVDTP_CODEC_SBC,caps,sizeof caps,configuration,sizeof configuration);
 if(!ep)return false;
 seid=avdtp_local_seid(ep);avdtp_set_preferred_sampling_frequency(ep,48000);
 avrcp_init();avrcp_register_packet_handler(remote_handler);avrcp_target_init();avrcp_target_register_packet_handler(remote_handler);
 a2dp_source_create_sdp_record(source_record,sdp_create_service_record_handle(),AVDTP_SOURCE_FEATURE_MASK_PLAYER,NULL,NULL);sdp_register_service(source_record);
 avrcp_target_create_sdp_record(target_record,sdp_create_service_record_handle(),AVRCP_FEATURE_MASK_CATEGORY_PLAYER_OR_RECORDER,NULL,NULL);sdp_register_service(target_record);
 registration.callback=hci_handler;hci_add_event_handler(&registration);
 gap_set_local_name("PicoMado");gap_discoverable_control(0);gap_ssp_set_io_capability(SSP_IO_CAPABILITY_NO_INPUT_NO_OUTPUT);
 hci_set_inquiry_mode(INQUIRY_MODE_RSSI_AND_EIR);
 timer.process=tick;connect_timer.process=retry_connect;recovery_timer.process=recovery_poll;init_ok=hci_power_control(HCI_POWER_ON)==0;return init_ok;
}
uint8_t pm_bt_runtime_scan(uint64_t gen){
 if(!ready||stopping||scanning||wanted||cid||!gen)return ERROR_CODE_COMMAND_DISALLOWED;
 generation=gen;uint8_t status=gap_inquiry_start(5);scanning=!status;return status;
}
#include "bluetooth_refresh.inc"
uint8_t pm_bt_runtime_connect(uint64_t gen,const uint8_t address[6]){
 return pm_bt_runtime_connect_at(gen,address,btstack_run_loop_get_time_ms());
}
uint8_t pm_bt_runtime_connect_at(uint64_t gen,const uint8_t address[6],uint32_t received_ms){
 if(!ready||stopping||scanning||wanted||cid||!gen||!address)return ERROR_CODE_COMMAND_DISALLOWED;
 if((uint32_t)(btstack_run_loop_get_time_ms()-received_ms)>=25000)return ERROR_CODE_COMMAND_DISALLOWED;
 generation=gen;memcpy(peer,address,6);wanted=true;retry_cancel();recovery_cancel();
 pm_bt_recovery_start(&recovery,gen,peer,received_ms);
 btstack_run_loop_set_timer(&recovery_timer,250);btstack_run_loop_add_timer(&recovery_timer);
 uint8_t status=a2dp_source_establish_stream(peer,&cid);
 if(status==ERROR_CODE_ACL_CONNECTION_ALREADY_EXISTS){
  avdtp_connection_t *existing=avdtp_get_connection_for_bd_addr(peer);
  cid=existing?existing->avdtp_cid:0;
  emit((pm_bt_event){.kind=PM_BT_LINK_DIAGNOSTIC,.status=status,.detail1=cid,.detail3=3});
  retry_schedule();return 0; /* accepted pending work, never CONNECTED */
 }
 if(status){recovery_cancel();wanted=false;cid=0;}return status;
}
void pm_bt_runtime_stop(uint64_t gen){
 stopping=true;stop_generation=gen;retry_cancel();recovery_cancel();
 wanted=false;media_stop();encoder=NULL;rate=0;if(remote_cid)avrcp_disconnect(remote_cid);if(scanning){gap_inquiry_stop();}
 if(cid)a2dp_source_disconnect(cid);
 stopped_if_idle();
}

bool pm_bt_runtime_audio_active(void){return audio.active;}

void pm_bt_runtime_link_snapshot(pm_bt_link_snapshot *out){
 if(!out)return;memset(out,0,sizeof *out);out->cid=cid;out->generation=generation;out->endpoint=-1;out->role=out->mode=0xff;out->handle=0xffff;
 avdtp_connection_t *c=cid?avdtp_get_connection_for_avdtp_cid(cid):NULL;
 if(!c)return;
 pm_bt_setup_progress progress=setup_progress(c);
 out->configuration=progress.configuration;out->endpoint_valid=progress.endpoint_valid;
 out->endpoint_initiator=progress.endpoint_initiator;out->endpoint_acceptor=progress.endpoint_acceptor;
 out->local_seid=progress.local_seid;out->active_matches=progress.active_matches;
 out->initiator_label=progress.initiator_label;out->acceptor_label=progress.acceptor_label;
 out->initiator_signal=progress.initiator_signal;out->acceptor_signal=progress.acceptor_signal;
 out->initiator_packet_label=progress.initiator_packet_label;out->acceptor_packet_label=progress.acceptor_packet_label;
 out->handle=c->con_handle;
 hci_connection_t *h=hci_connection_for_handle(c->con_handle);
 if(h){out->role=h->role;out->mode=h->connection_mode;}
 out->packet_types_known=observed_packet_valid&&observed_packet_handle==c->con_handle;
 if(out->packet_types_known)out->packet_types=observed_packet_types;
 out->interval_known=observed_mode_valid&&observed_mode_handle==c->con_handle;
 if(out->interval_known)out->mode_interval=observed_mode_interval;
 out->exists=true;out->connection=c->state;out->signaling_cid=c->l2cap_signaling_cid;
 out->initiator=c->initiator_connection_state;
 a2dp_config_process_t *p=&c->a2dp_source_config_process;
 out->source=p->state;out->outgoing=p->outgoing_active;out->discover=p->discover_seps;
 out->configured=p->stream_endpoint_configured;out->have_config=p->have_config;
 if(p->local_stream_endpoint){out->endpoint=p->local_stream_endpoint->state;out->media_cid=p->local_stream_endpoint->l2cap_media_cid;}
}
