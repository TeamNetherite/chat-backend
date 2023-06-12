use crate::{
    model::{
        guild::{Channel, Guild},
        message::Message,
        user::User,
    },
    util::{Cx, Referrable},
};
use async_graphql::*;

pub struct ById;

#[Object]
impl ById {
    async fn user(&self, cx: &Context<'_>, id: ID) -> Result<Option<User>> {
        let user: Option<User> = cx
            .cx()
            .surreal()
            .select((User::TABLE, id.0.as_str()))
            .await?;
        Ok(user)
    }

    async fn message(&self, cx: &Context<'_>, id: ID) -> Result<Option<Message>> {
        let message: Option<Message> = cx
            .cx()
            .surreal()
            .select((Message::TABLE, id.0.as_str()))
            .await?;
        Ok(message)
    }

    async fn channel(&self, cx: &Context<'_>, id: ID) -> Result<Option<Channel>> {
        let channel: Option<Channel> = cx
            .cx()
            .surreal()
            .select((Channel::TABLE, id.0.as_str()))
            .await?;
        Ok(channel)
    }

    async fn guild(&self, cx: &Context<'_>, id: ID) -> Result<Option<Guild>> {
        let guild: Option<Guild> = cx
            .cx()
            .surreal()
            .select((Guild::TABLE, id.0.as_str()))
            .await?;
        Ok(guild)
    }
}
