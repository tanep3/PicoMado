/* BT-AUDIO-20261107: shared-lock audio adapter, also exercised by host fixtures. */
static void schedule_audio(uint32_t delay){
 if(timer_scheduled)btstack_run_loop_remove_timer(&timer);
 timer_due_ms=btstack_run_loop_get_time_ms()+delay;timer_scheduled=true;
 btstack_run_loop_set_timer(&timer,delay);btstack_run_loop_add_timer(&timer);
}
static void media_stop(void){
 streaming=false;pending=false;prepared_count=prepared_bytes=0;timer_scheduled=false;btstack_run_loop_remove_timer(&timer);pm_bluetooth_audio_stop(&audio);
 pm_bt_rebuffer_reset(&rebuffer,0,0);
}
static unsigned ready_packet(void){
 unsigned frames=audio.sbc?128:encoder->num_audio_frames(&encoder_state);
 unsigned reserve=audio.sbc?pm_bt_sbc_frame_bytes(&audio.sbc_config):pm_bt_sbc_bytes((unsigned)encoder_state.params.s16BitPool,
     encoder_state.params.s16ChannelMode==SBC_CHANNEL_MODE_JOINT_STEREO);
 int mtu=a2dp_max_media_payload_size(cid,seid);
 unsigned target=pm_bt_packet_frames(rate,frames,reserve,mtu>0?(size_t)mtu:0,sizeof encoded);
 return pm_bt_ready_packet(target,frames,audio.used,pcm_eof,accepted_frames,pcm_end);
}
static bool audio_scope_current(void){
 return audio.active&&pm_bt_rebuffer_current(&rebuffer,generation,audio.generation)&&
     (!audio.sbc||(audio.codec_epoch==codec_epoch&&pm_bt_sbc_config_equal(&audio.sbc_config,&codec_config)));
}
static void rebuffer_fail(pm_bt_rebuffer_result result,uint32_t now){
 diagnostics.rebuffer_failed++;
 emit((pm_bt_event){.kind=PM_BT_UNDERFLOW,.status=0xff04,
      .detail1=(int32_t)result,.detail2=(int32_t)(now-rebuffer.since),.detail3=(int32_t)audio.used});
 media_stop();if(cid&&wanted)a2dp_source_pause_stream(cid,seid);
}
static bool rebuffer_timed_out(uint32_t now){
 if(rebuffer.shortage&&(uint32_t)(now-rebuffer.since)>=PM_BT_REBUFFER_TIMEOUT_MS){
  rebuffer_fail(PM_BT_RB_TIMEOUT,now);return true;
 }
 return false;
}
static void tick(btstack_timer_source_t *t){
 (void)t;if(!wanted||!streaming||!encoder)return;
 if(!audio_scope_current()){media_stop();return;}
 uint32_t now=btstack_run_loop_get_time_ms();
 if(timer_scheduled){int32_t late=(int32_t)(now-timer_due_ms);timer_scheduled=false;
  if(late>0){diagnostics.timer_late_total_ms+=(uint32_t)late;if((uint32_t)late>diagnostics.timer_late_max_ms)diagnostics.timer_late_max_ms=(uint32_t)late;}}
 if(rebuffer_timed_out(now))return;
 if(pending){if(rebuffer.buffering)schedule_audio(5);return;}
 if(!prepared_count){
  if(pcm_eof&&accepted_frames==pcm_end&&consumed_frames==pcm_end){
   int32_t tail=clock_started?(int32_t)(last_packet_end_ms-now):0;
   if(tail>0){schedule_audio((uint32_t)tail);return;}
   media_stop();return;
  }
  bool packet_ready=ready_packet()!=0;
  bool refilled=audio.used>=pm_bluetooth_audio_capacity(&audio) || (pcm_eof&&accepted_frames==pcm_end&&packet_ready);
  bool missed=!clock_started||pm_bt_packet_delay(deadline,samples_sent,rate,now)<0;
  bool was_buffering=rebuffer.buffering;
  pm_bt_rebuffer_result result=pm_bt_rebuffer_step(&rebuffer,now,packet_ready,refilled,missed);
  if(!was_buffering&&rebuffer.buffering)diagnostics.rebuffer_count++;
  if(result==PM_BT_RB_TIMEOUT||result==PM_BT_RB_EXHAUSTED){rebuffer_fail(result,now);return;}
  if(result==PM_BT_RB_WAIT){schedule_audio(5);return;}
  prepare_audio();
 }
 if(!streaming||!prepared_count)return;
 /* Only an explicit completed rebuffer may reanchor. Normal packets retain
  * the original absolute48kHz schedule; no catch-up burst is introduced. */
 if(clock_started&&!rebuffer.resuming){
  int32_t delay=pm_bt_packet_delay(deadline,samples_sent,rate,btstack_run_loop_get_time_ms());
  if(delay>0){schedule_audio((uint32_t)delay);return;}
 }
 if(!pending){
  pending=true;can_requested_ms=btstack_run_loop_get_time_ms();
  a2dp_source_stream_endpoint_request_can_send_now(cid,seid);
  /* Grant may be synchronous; never overwrite the successful send's timer. */
  if(pending&&rebuffer.buffering)schedule_audio(5);
 }
}
static void prepare_audio(void){
 if(!streaming||!wanted||!encoder||prepared_count)return;
 if(!audio_scope_current()){media_stop();return;}
 uint32_t send_started=btstack_run_loop_get_time_ms();
 unsigned frames=audio.sbc?128:encoder->num_audio_frames(&encoder_state);
 int mtu=a2dp_max_media_payload_size(cid,seid);
 int16_t pcm[256];
 /* Compute exact encoded size from negotiated16x8/stereo and chosen bitpool;
  * SDK packet length is still zero before the first encode. */
 unsigned bytes;
 unsigned reserve=audio.sbc?pm_bt_sbc_frame_bytes(&audio.sbc_config):pm_bt_sbc_bytes((unsigned)encoder_state.params.s16BitPool,
     encoder_state.params.s16ChannelMode==SBC_CHANNEL_MODE_JOINT_STEREO);
 unsigned target=pm_bt_packet_frames(rate,frames,reserve,mtu>0?(size_t)mtu:0,sizeof encoded);
 if(frames!=128||!target){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff01,.detail1=(int32_t)frames,.detail2=(int32_t)reserve,.detail3=mtu});media_stop();return;}
 unsigned eligible=pm_bt_ready_packet(target,frames,audio.used,pcm_eof,accepted_frames,pcm_end);
 if(!eligible){schedule_audio(5);return;}
 target=eligible;
 unsigned count=0,used=1;
 while(count<target&&audio.used>=frames){
  if(audio.sbc){
   if(!pm_bluetooth_audio_sbc_pop(&audio,encoded+used,sizeof encoded-used)){media_stop();return;}
   used+=reserve;count++;continue;
  }
  if(!pm_bluetooth_audio_pop(&audio,pcm,frames))break;
  uint8_t status=encoder->encode_signed_16(&encoder_state,pcm,encoded+used);
  bytes=encoder->sbc_buffer_length(&encoder_state);
  if(status||!bytes||bytes>reserve){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff01,.detail1=status,.detail2=(int32_t)bytes,.detail3=mtu});media_stop();return;}
  used+=bytes;count++;
 }
 uint32_t encode_ms=btstack_run_loop_get_time_ms()-send_started;
 diagnostics.encode_total_ms+=encode_ms;if(encode_ms>diagnostics.encode_max_ms)diagnostics.encode_max_ms=encode_ms;
 diagnostics.last_count=count;diagnostics.last_bytes=used;diagnostics.mtu=(uint32_t)mtu;
 encoded[0]=(uint8_t)count;
 prepared_count=count;prepared_bytes=used;
}
static void send_audio(void){
 if(!pending)return;
 pending=false;if(!streaming||!wanted||!encoder||!prepared_count)return;
 if(!audio_scope_current()){media_stop();return;}
 if(rebuffer_timed_out(btstack_run_loop_get_time_ms()))return;
 uint32_t send_started=btstack_run_loop_get_time_ms();
 uint32_t wait=send_started-can_requested_ms;diagnostics.can_wait_total_ms+=wait;
 if(wait>diagnostics.can_wait_max_ms)diagnostics.can_wait_max_ms=wait;
 unsigned count=prepared_count,used=prepared_bytes,frames=128;
 int mtu=a2dp_max_media_payload_size(cid,seid);
 uint8_t status=a2dp_source_stream_send_media_payload_rtp(cid,seid,0,timestamp,encoded,used);
 if(status){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=status,.detail1=(int32_t)used,.detail2=mtu,.detail3=(int32_t)(btstack_run_loop_get_time_ms()-send_started)});media_stop();return;}
 uint32_t now=btstack_run_loop_get_time_ms();
 /* Anchor to first successful transmission, excluding asynchronous START /
  * can-send latency. Thereafter never silently reset the sample clock. */
 if(!clock_started||rebuffer.resuming){
  if(rebuffer.resuming){
   uint32_t waited=now-rebuffer.since;diagnostics.rebuffer_total_ms+=waited;
   if(waited>diagnostics.rebuffer_max_ms)diagnostics.rebuffer_max_ms=waited;
  }
  deadline=now-(uint32_t)(samples_sent*1000/rate);clock_started=true;
 }
 pm_bt_rebuffer_sent(&rebuffer,now);
 prepared_count=prepared_bytes=0;
 if(audio.sbc&&!pm_bluetooth_audio_sbc_sent(&audio,count*frames)){media_stop();return;}
 consumed_frames+=count*frames; /* Credit/completion never outruns successful RTP send. */
 diagnostics.packets++;diagnostics.frames+=count*frames;
 timestamp+=count*frames;samples_sent+=count*frames;
 last_packet_end_ms=now+(count*frames*1000u+rate-1u)/rate;
 int32_t delay=pm_bt_packet_delay(deadline,samples_sent,rate,now);
 if(delay < -250){emit((pm_bt_event){.kind=PM_BT_ERROR,.status=0xff02,.detail1=delay,.detail2=(int32_t)(now-send_started),.detail3=(int32_t)samples_sent});media_stop();return;}
 /* Separate turn: no recursive encoding/request chain from synchronous grants. */
 schedule_audio(1);
}
uint32_t pm_bt_runtime_rate(void){return rate;}
void pm_bt_runtime_enable_input(bool enabled){input_enabled=enabled;input_epoch++;}
bool pm_bt_runtime_audio_begin(uint64_t gen,uint64_t first_frame){
 if(!wanted||!cid||!encoder||!gen)return false;
 if(streaming)return false;
 if(!pm_bluetooth_audio_start(&audio,gen,rate))return false;
 memset(&diagnostics,0,sizeof diagnostics);diagnostics.bitpool=(uint32_t)encoder_state.params.s16BitPool;timer_scheduled=false;
 prepared_count=prepared_bytes=0;
 pm_bt_rebuffer_reset(&rebuffer,generation,gen);
 pcm_eof=false;pcm_end=0;pcm_first=first_frame;last_packet_end_ms=0;
 audio.next_frame=first_frame;accepted_frames=consumed_frames=first_frame;return true;
}
bool pm_bt_runtime_audio_finish(uint64_t gen,uint64_t end){
 if(!audio_scope_current()||audio.generation!=gen||end<accepted_frames||end<pcm_first||(end-pcm_first)%128)return false;
 if(pcm_eof&&end!=pcm_end)return false;
 pcm_eof=true;pcm_end=end;return true;
}
pm_bluetooth_audio_result pm_bt_runtime_audio_push(uint64_t gen,uint64_t frame,const int16_t *pcm,size_t frames){
 if(!audio_scope_current())return PM_BT_PCM_INACTIVE;
 if(pcm_eof&&(frame>pcm_end||frames>pcm_end-frame))return PM_BT_PCM_INVALID;
 pm_bluetooth_audio_result result=pm_bluetooth_audio_push(&audio,gen,frame,pcm,frames);
 if(result==PM_BT_PCM_OK)accepted_frames=audio.next_frame;
 return result;
}
uint64_t pm_bt_runtime_audio_next_frame(void){return accepted_frames;}
uint64_t pm_bt_runtime_audio_consumed_frames(void){return consumed_frames;}
size_t pm_bt_runtime_audio_capacity(void){return pm_bluetooth_audio_capacity(&audio);}
size_t pm_bt_runtime_audio_free_frames(void){return audio.active?pm_bluetooth_audio_capacity(&audio)-(accepted_frames-consumed_frames):0;}
void pm_bt_runtime_audio_end(void){media_stop();if(cid&&wanted)a2dp_source_pause_stream(cid,seid);}
uint8_t pm_bt_runtime_play(void){
 if(!wanted||!encoder||!audio_scope_current()||audio.used<128)return ERROR_CODE_COMMAND_DISALLOWED;
 return a2dp_source_start_stream(cid,seid);
}

bool pm_bt_runtime_audio_sbc_begin(uint64_t gen,uint64_t first,pm_bt_sbc_config *config,uint64_t *epoch){
 if(!config||!epoch||!wanted||!cid||!encoder||streaming||!gen||!codec_epoch||!pm_bt_sbc_config_valid(&codec_config))return false;
 if(!pm_bt_runtime_audio_begin(gen,first))return false;
 if(!pm_bluetooth_audio_sbc_start(&audio,gen,first,codec_epoch,&codec_config)){media_stop();return false;}
 *config=audio.sbc_config;*epoch=audio.codec_epoch;return true;
}
pm_bluetooth_audio_result pm_bt_runtime_audio_sbc_push(uint64_t gen,uint64_t first,const uint8_t *bytes,size_t n){
 if(!audio_scope_current())return PM_BT_PCM_INACTIVE;
 size_t frames;
 if(!audio.sbc||!n)return PM_BT_PCM_INVALID;
 unsigned frame_bytes=pm_bt_sbc_frame_bytes(&audio.sbc_config);
 if(n%frame_bytes||n/frame_bytes>PM_BT_SBC_CHUNK)return PM_BT_PCM_INVALID;
 frames=n/frame_bytes*128; /* Full CRC/header validation is atomic in the aggregate below. */
 if(pcm_eof&&(first>pcm_end||frames>pcm_end-first))return PM_BT_PCM_INVALID;
 pm_bluetooth_audio_result r=pm_bluetooth_audio_sbc_push(&audio,gen,codec_epoch,first,bytes,n);
 if(r==PM_BT_PCM_OK)accepted_frames=audio.next_frame;
 return r;
}
