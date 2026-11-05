//! Shared monotonic playback position, independent per-terminal admission.
//! Frame IO happens without the App mutex; generation gates reject old reads.

use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::watch;

use crate::application::AppError;
use crate::web::SharedApp;
use crate::workers::Executors;

pub async fn run_playback(
    app: SharedApp,
    executors: Executors,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), AppError> {
    crate::eink_video::recover_stale_work();
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    let mut diagnostic_tick=0;
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sent = BTreeMap::<String, (crate::DisplayToken, String, u64, u64, u64)>::new();
    let mut conversion_retry_after = BTreeMap::<String, u64>::new();
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        tokio::select! {
            changed=shutdown.changed()=>if changed.is_err() || *shutdown.borrow() {return Ok(());},
            _=tick.tick()=>{
                crate::diagnostics::tick(crate::diagnostics::Domain::Playback,&mut diagnostic_tick);
                let (now,offers)={let guard=crate::diagnostics::read(&app, crate::diagnostics::Domain::Playback).map_err(|_|AppError::Conflict("application unavailable"))?;
                    (guard.monotonic_millis(),guard.playback_offers())};
                for mut offer in offers {
                    let sequence=now.saturating_sub(offer.spec.started_mono_ms)
                        .saturating_mul(offer.spec.fps as u64)/1000;
                    offer.targets.retain(|target|{
                        if target.eink.is_some() && conversion_retry_after.get(&offer.plugin)
                            .is_some_and(|until| now < *until) { return false; }
                        let Some((token,plugin,generation,last_sequence,last_at))=sent.get(&target.token.device_id) else{return true};
                        if token!=&target.token || plugin!=&offer.plugin || *generation!=offer.spec.generation{return true}
                        if *last_sequence==sequence{return false}
                        if let Some(settings)=target.eink {
                            let interval=if target.eink_partial {1000/u64::from(settings.fps.min(offer.spec.fps as u8))}
                                else {u64::from(settings.full_refresh_seconds)*1000};
                            return now.saturating_sub(*last_at)>=interval;
                        }
                        true
                    });
                    if offer.targets.is_empty(){continue;}
                    let Some(executor)=executors.get(&offer.plugin) else{continue;};
                    let paths=crate::diagnostics::read(&app, crate::diagnostics::Domain::Playback).map_err(|_|AppError::Conflict("application unavailable"))?
                        .plugin_paths(&offer.plugin).ok_or(AppError::NotFound)?;
                    let read=executor.read_frame(offer.spec.clone(),sequence%offer.spec.frame_count,paths);
                    tokio::pin!(read);
                    let pixels=tokio::select! {
                        result=&mut read=>match result {Ok(pixels)=>pixels,Err(_)=>continue},
                        changed=shutdown.changed()=>{
                            if changed.is_err() || *shutdown.borrow(){return Ok(());}
                            continue;
                        }
                    };
                    let mut eink_pixels=std::collections::BTreeMap::new();
                    for auto_preprocess in [true,false] {
                        if !offer.targets.iter().any(|t|t.eink.is_some_and(|s|s.auto_preprocess==auto_preprocess)) {continue;}
                        let source=pixels.clone();
                        match tokio::task::spawn_blocking(move||crate::eink_video::convert_lcd_with_preprocess(&source,auto_preprocess)).await {
                            Ok(Ok(mono)) => { conversion_retry_after.remove(&offer.plugin); eink_pixels.insert(auto_preprocess,mono); }
                            result => {
                                let reason = match result { Ok(Err(reason)) => reason, Err(_) => "conversion worker failed", _ => unreachable!() };
                                eprintln!("E-ink video conversion failed for {}: {reason}; retry in 5 seconds",offer.plugin);
                                conversion_retry_after.insert(offer.plugin.clone(),now.saturating_add(5000));
                            }
                        }
                    }
                    let targets=std::mem::take(&mut offer.targets);
                    for target in targets {
                        let id=target.token.device_id.clone();
                        let token=target.token.clone();
                        let mono=target.eink.and_then(|settings|eink_pixels.get(&settings.auto_preprocess));
                        offer.targets=vec![target];
                        if app.accept_playback_pixels_with_eink_at(&offer,pixels.clone(),mono.map(Vec::as_slice),sequence)? > 0 {
                            sent.insert(id,(token,offer.plugin.clone(),offer.spec.generation,sequence,now));
                        }
                    }
                }
            }
        }
    }
}
