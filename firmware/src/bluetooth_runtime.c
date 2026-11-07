/* BT-AUDIO-20261107: bounded shared-runtime adapter. Hardware validation remains required. */
#include "bluetooth_runtime.h"
#include "btstack.h"
#include "classic/btstack_sbc_bluedroid.h"
static pm_bt_event queue[8];
static unsigned rd, count;
static uint32_t dropped;
static uint64_t generation, stop_generation;
static bool stopping;
static bool init_ok, initialized, ready, wanted, scanning;
static bd_addr_t peer;
static uint16_t cid;
static uint8_t seid;
static uint16_t remote_cid;
static uint32_t rate, timestamp, deadline;
static uint64_t accepted_frames, consumed_frames, samples_sent;
static bool streaming, pending, input_enabled;
static bool pressed[128];
static pm_bluetooth_audio audio;
static btstack_timer_source_t timer;
static btstack_sbc_encoder_bluedroid_t encoder_state;
static const btstack_sbc_encoder_t *encoder;
static uint8_t encoded[256];
static void tick(btstack_timer_source_t *t);
static uint8_t caps[]={0x33,0x15,2,53}, configuration[4];
static uint8_t source_record[150], target_record[200];
static btstack_packet_callback_registration_t registration;
static void emit(pm_bt_event e){
 if(!e.generation)e.generation=generation;
 if(count==8){if(dropped!=UINT32_MAX)++dropped;return;}
 queue[(rd+count)%8]=e;++count;
}
bool pm_bt_runtime_next(pm_bt_event *e){if(!e||!count)return false;*e=queue[rd];rd=(rd+1)%8;--count;return true;}
uint32_t pm_bt_runtime_dropped(void){return dropped;}
static void media_stop(void){
 streaming=false;pending=false;btstack_run_loop_remove_timer(&timer);pm_bluetooth_audio_stop(&audio);
}
static void tick(btstack_timer_source_t *t){
 (void)t;if(!wanted||!streaming||!encoder)return;
 if(!pending){pending=true;a2dp_source_stream_endpoint_request_can_send_now(cid,seid);}
}
static void send_audio(void){
 pending=false;if(!streaming||!wanted||!encoder)return;
 unsigned frames=encoder->num_audio_frames(&encoder_state);
 unsigned bytes=encoder->sbc_buffer_length(&encoder_state);
 int16_t pcm[256];
 if(frames>128||bytes+1>sizeof encoded||bytes+1>(unsigned)a2dp_max_media_payload_size(cid,seid)){
  emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff01});media_stop();return;
 }
 if(!pm_bluetooth_audio_pop(&audio,pcm,frames)){
  emit((pm_bt_event){.kind=PM_BT_UNDERFLOW});media_stop();a2dp_source_pause_stream(cid,seid);return;
 }
 consumed_frames+=frames;
 encoded[0]=1;encoder->encode_signed_16(&encoder_state,pcm,encoded+1);
 uint8_t status=a2dp_source_stream_send_media_payload_rtp(cid,seid,0,timestamp,encoded,bytes+1);
 if(status){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=status});media_stop();return;}
 timestamp+=frames;samples_sent+=frames;
 /* Absolute sample clock avoids millisecond rounding drift. No catch-up burst. */
 uint32_t now=btstack_run_loop_get_time_ms();
 uint32_t due=deadline+(uint32_t)((samples_sent*1000)/rate);
 int32_t delay=(int32_t)(due-now);
 if(delay < -100){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff02});media_stop();return;}
 btstack_run_loop_set_timer(&timer,delay>0?(uint32_t)delay:1);btstack_run_loop_add_timer(&timer);
}
uint32_t pm_bt_runtime_rate(void){return rate;}
void pm_bt_runtime_enable_input(bool enabled){input_enabled=enabled;memset(pressed,0,sizeof pressed);}
bool pm_bt_runtime_audio_begin(uint64_t gen,uint64_t first_frame){
 if(!wanted||!cid||!encoder||!gen)return false;
 if(streaming)return false;
 if(!pm_bluetooth_audio_start(&audio,gen,rate))return false;
 audio.next_frame=first_frame;accepted_frames=consumed_frames=first_frame;return true;
}
pm_bluetooth_audio_result pm_bt_runtime_audio_push(uint64_t gen,uint64_t frame,const int16_t *pcm,size_t frames){
 pm_bluetooth_audio_result result=pm_bluetooth_audio_push(&audio,gen,frame,pcm,frames);
 if(result==PM_BT_PCM_OK)accepted_frames=audio.next_frame;return result;
}
uint64_t pm_bt_runtime_audio_next_frame(void){return accepted_frames;}
uint64_t pm_bt_runtime_audio_consumed_frames(void){return consumed_frames;}
size_t pm_bt_runtime_audio_free_frames(void){return audio.active?PM_BT_PCM_FRAMES-audio.used:0;}
void pm_bt_runtime_audio_end(void){media_stop();if(cid&&wanted)a2dp_source_pause_stream(cid,seid);}
uint8_t pm_bt_runtime_play(void){
 if(!wanted||!encoder||!audio.active||audio.used<128)return ERROR_CODE_COMMAND_DISALLOWED;
 return a2dp_source_start_stream(cid,seid);
}
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
static void source_handler(uint8_t type,uint16_t channel,uint8_t *p,uint16_t size){
 (void)channel;(void)size;if(type!=HCI_EVENT_PACKET||hci_event_packet_get_type(p)!=HCI_EVENT_A2DP_META)return;
 switch(p[2]){
 case A2DP_SUBEVENT_SIGNALING_CONNECTION_ESTABLISHED:{
  uint16_t c=a2dp_subevent_signaling_connection_established_get_a2dp_cid(p);
  uint8_t status=a2dp_subevent_signaling_connection_established_get_status(p);
  if(c!=cid){if(!status)a2dp_source_disconnect(c);return;}
  if(!wanted){if(!status)a2dp_source_disconnect(c);else{cid=0;stopped_if_idle();}return;}
  if(status){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=status});cid=0;wanted=false;}
  break;}
 case A2DP_SUBEVENT_SIGNALING_MEDIA_CODEC_SBC_CONFIGURATION:{
  if(!wanted||a2dp_subevent_signaling_media_codec_sbc_configuration_get_a2dp_cid(p)!=cid)return;
  rate=a2dp_subevent_signaling_media_codec_sbc_configuration_get_sampling_frequency(p);
  unsigned channels=a2dp_subevent_signaling_media_codec_sbc_configuration_get_num_channels(p);
  unsigned mode=a2dp_subevent_signaling_media_codec_sbc_configuration_get_channel_mode(p);
  if(channels!=2||(rate!=44100&&rate!=48000)||(mode!=AVDTP_CHANNEL_MODE_STEREO&&mode!=AVDTP_CHANNEL_MODE_JOINT_STEREO)){
   emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff03});pm_bt_runtime_stop(generation);return;
  }
  encoder=btstack_sbc_encoder_bluedroid_init_instance(&encoder_state);
  encoder->configure(&encoder_state,SBC_MODE_STANDARD,
   a2dp_subevent_signaling_media_codec_sbc_configuration_get_block_length(p),
   a2dp_subevent_signaling_media_codec_sbc_configuration_get_subbands(p),
   (btstack_sbc_allocation_method_t)(a2dp_subevent_signaling_media_codec_sbc_configuration_get_allocation_method(p)-1),rate,
   a2dp_subevent_signaling_media_codec_sbc_configuration_get_max_bitpool_value(p),
   mode==AVDTP_CHANNEL_MODE_STEREO?SBC_CHANNEL_MODE_STEREO:SBC_CHANNEL_MODE_JOINT_STEREO);
  emit((pm_bt_event){.kind=PM_BT_CODEC,.rate=rate});break;}
 case A2DP_SUBEVENT_STREAM_STARTED:
  if(!wanted||a2dp_subevent_stream_started_get_a2dp_cid(p)!=cid)return;
  if(!audio.active){a2dp_source_pause_stream(cid,seid);return;}
  streaming=true;timestamp=0;samples_sent=0;deadline=btstack_run_loop_get_time_ms();tick(&timer);break;
 case A2DP_SUBEVENT_STREAMING_CAN_SEND_MEDIA_PACKET_NOW:
  if(a2dp_subevent_streaming_can_send_media_packet_now_get_a2dp_cid(p)==cid)send_audio();break;
 case A2DP_SUBEVENT_STREAM_SUSPENDED:
  if(little_endian_read_16(p,3)==cid&&streaming)media_stop();break;
 case A2DP_SUBEVENT_STREAM_RELEASED:
  if(little_endian_read_16(p,3)==cid)media_stop();break;

 case A2DP_SUBEVENT_STREAM_ESTABLISHED:{
  uint16_t c=a2dp_subevent_stream_established_get_a2dp_cid(p);
  if(c!=cid)return;
  uint8_t status=a2dp_subevent_stream_established_get_status(p);
  if(!wanted){a2dp_source_disconnect(c);return;}
  emit((pm_bt_event){.kind=status?PM_BT_ERROR:PM_BT_CONNECTED,.status=status});
  if(!status&&!remote_cid){uint16_t rc=0;if(!avrcp_connect(peer,&rc))remote_cid=rc;}
  break;}
 case A2DP_SUBEVENT_SIGNALING_CONNECTION_RELEASED:
  if(a2dp_subevent_signaling_connection_released_get_a2dp_cid(p)!=cid)return;
  media_stop();encoder=NULL;rate=0;cid=0;wanted=false;emit((pm_bt_event){.kind=PM_BT_DISCONNECTED});stopped_if_idle();break;
 }
}
static void remote_handler(uint8_t type,uint16_t channel,uint8_t *p,uint16_t size){
 (void)channel;(void)size;if(type!=HCI_EVENT_PACKET||hci_event_packet_get_type(p)!=HCI_EVENT_AVRCP_META)return;
 if(p[2]==AVRCP_SUBEVENT_CONNECTION_ESTABLISHED){
  bd_addr_t addr;avrcp_subevent_connection_established_get_bd_addr(p,addr);
  uint16_t c=avrcp_subevent_connection_established_get_avrcp_cid(p);
  if(avrcp_subevent_connection_established_get_status(p)){if(c==remote_cid)remote_cid=0;stopped_if_idle();return;}
  if(!wanted||memcmp(addr,peer,6)){avrcp_disconnect(c);return;}remote_cid=c;return;
 }
 if(p[2]==AVRCP_SUBEVENT_CONNECTION_RELEASED){if(avrcp_subevent_connection_released_get_avrcp_cid(p)==remote_cid)remote_cid=0;memset(pressed,0,sizeof pressed);stopped_if_idle();return;}
 if(p[2]!=AVRCP_SUBEVENT_OPERATION)return;
 uint16_t c=avrcp_subevent_operation_get_avrcp_cid(p);
 uint8_t op=avrcp_subevent_operation_get_operation_id(p);
 bool down=avrcp_subevent_operation_get_button_pressed(p)!=0;
 bool supported=op==AVRCP_OPERATION_ID_PLAY||op==AVRCP_OPERATION_ID_PAUSE||op==AVRCP_OPERATION_ID_FORWARD||op==AVRCP_OPERATION_ID_BACKWARD;
 if(!wanted||c!=remote_cid||!input_enabled||!supported||count==8){return;}
 /* SDK auto-acknowledges valid opcodes before this callback. */
 if(pressed[op]==down)return;
 pressed[op]=down;emit((pm_bt_event){.kind=PM_BT_INPUT,.operation=op,.pressed=down});
}
bool pm_bt_runtime_init(void){
 if(initialized)return init_ok;
 initialized=true;
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
 timer.process=tick;init_ok=hci_power_control(HCI_POWER_ON)==0;return init_ok;
}
uint8_t pm_bt_runtime_scan(uint64_t gen){
 if(!ready||stopping||scanning||wanted||cid||!gen)return ERROR_CODE_COMMAND_DISALLOWED;
 generation=gen;uint8_t status=gap_inquiry_start(5);scanning=!status;return status;
}
uint8_t pm_bt_runtime_connect(uint64_t gen,const uint8_t address[6]){
 if(!ready||stopping||scanning||wanted||cid||!gen||!address)return ERROR_CODE_COMMAND_DISALLOWED;
 generation=gen;memcpy(peer,address,6);wanted=true;
 uint8_t status=a2dp_source_establish_stream(peer,&cid);if(status){wanted=false;cid=0;}return status;
}
void pm_bt_runtime_stop(uint64_t gen){
 stopping=true;stop_generation=gen;
 wanted=false;media_stop();encoder=NULL;rate=0;memset(pressed,0,sizeof pressed);if(remote_cid)avrcp_disconnect(remote_cid);if(scanning){gap_inquiry_stop();}
 if(cid)a2dp_source_disconnect(cid);
 stopped_if_idle();
}

bool pm_bt_runtime_audio_active(void){return audio.active;}
