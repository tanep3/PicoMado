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
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sent = BTreeMap::<String, (crate::DisplayToken, String, u64, u64)>::new();
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        tokio::select! {
            changed=shutdown.changed()=>if changed.is_err() || *shutdown.borrow() {return Ok(());},
            _=tick.tick()=>{
                let (now,offers)={let guard=app.lock().map_err(|_|AppError::Conflict("application unavailable"))?;
                    (guard.monotonic_millis(),guard.playback_offers())};
                for mut offer in offers {
                    let sequence=now.saturating_sub(offer.spec.started_mono_ms)
                        .saturating_mul(offer.spec.fps as u64)/1000;
                    offer.targets.retain(|target|sent.get(&target.token.device_id)
                        != Some(&(target.token.clone(),offer.plugin.clone(),offer.spec.generation,sequence)));
                    if offer.targets.is_empty(){continue;}
                    let Some(executor)=executors.get(&offer.plugin) else{continue;};
                    let paths=app.lock().map_err(|_|AppError::Conflict("application unavailable"))?
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
                    let targets=std::mem::take(&mut offer.targets);
                    let mut guard=app.lock().map_err(|_|AppError::Conflict("application unavailable"))?;
                    for target in targets {
                        let id=target.token.device_id.clone();
                        let token=target.token.clone();
                        offer.targets=vec![target];
                        if guard.accept_playback_pixels_at(&offer,pixels.clone(),sequence)? > 0 {
                            sent.insert(id,(token,offer.plugin.clone(),offer.spec.generation,sequence));
                        }
                    }
                }
            }
        }
    }
}
