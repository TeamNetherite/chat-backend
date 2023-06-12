use std::convert::identity;

use async_graphql::*;

use crate::{
    model::{message::Message, user::User},
    util::{ReferrableWithId, ReferrableExt},
};

pub struct ManageMessage {
    capabilities: Vec<Capability>,
    user: User,
    message: Message,
}

impl ManageMessage {
    pub fn new(u: User, m: Message) -> Self {
        Self {
            capabilities: Capability::get_all(&u, &m),
            user: u,
            message: m,
        }
    }

    pub async fn _delete(&self) -> surrealdb::Result<Message> {
        Ok(crate::SURREAL.delete(self.message.record_id().0).await?)
    }
}

#[Object]
impl ManageMessage {
    async fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
    async fn delete(&self) -> Result<Message> {
        Ok(self._delete().await?)
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Capability {
    Delete,
    Edit,
}

impl Capability {
    pub fn get_all(u: &User, m: &Message) -> Vec<Self> {
        let is_author = <User as ReferrableWithId>::id(u).as_str() == m.author.id();

        [
            is_author.then_some(Self::Delete),
            is_author.then_some(Self::Edit),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}
