// Generated from the project observation model. No wildcard fallback.
use crate::protocol::WireMessage;
pub fn identity(message:&WireMessage)->(u64,u64,u8){
 let(s,f)=match message{
WireMessage::ReceiptAck{token,..} | WireMessage::RenderedAck{token,..} | WireMessage::StreamStatus{token,..} | WireMessage::FrameStart{token,..} | WireMessage::FrameChunk{token,..}=>(token.session_epoch,token.frame_id),
WireMessage::Welcome{session_epoch,..} | WireMessage::Up{session_epoch,..} | WireMessage::Down{session_epoch,..} | WireMessage::Next{session_epoch,..} | WireMessage::Power{session_epoch,..} | WireMessage::EnterSub{session_epoch,..} | WireMessage::Heartbeat{session_epoch,..}=>(*session_epoch,0),
WireMessage::Hello{..} | WireMessage::Settings{..} | WireMessage::SettingsStored{..} | WireMessage::InputAccepted{..} | WireMessage::Error{..} | WireMessage::PluginPosition{..}=>(0,0),
 };(s,f,message.kind())
}
