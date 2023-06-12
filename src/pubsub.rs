use async_std::{sync::RwLock, stream::Stream};
use flo_stream::{Publisher, MessagePublisher};

use crate::model::message::Message;

struct RelayInfo {
    pub sent_messages: RwLock<Publisher<Message>>
}

pub struct Relay {
    info: RelayInfo,
}

impl Relay {
    pub fn new() -> Relay {
        Relay {
            info: RelayInfo { sent_messages: RwLock::new(Publisher::new(30)) }
        }
    }

    pub async fn send_message(&self, message: &Message) {
        self.info.sent_messages.write().await.publish(message.clone()).await
    }

    pub async fn stream_sent_messages(&self) -> impl Stream<Item = Message> {
        self.info.sent_messages.write().await.subscribe()
    }
}
