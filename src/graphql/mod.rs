#![allow(unused_variables)]
pub mod guild;
mod loaders;
pub mod manage;
pub mod message;
pub mod user;

use async_graphql::{Result as FieldResult, *};
use async_std::future;
use futures_util::{Stream, StreamExt};
use serde::Deserialize;

use crate::{
    http::SURREAL,
    model::{
        guild::{Guild, GuildInit},
        message::{Conversation, Message, MessageInit, MessageRecipient},
        user::{parse_tag, Status, User, Theme},
    },
    util::{Cx, RecordId, Ref, ReferrableExt},
};

use self::{loaders::ById, manage::ManageMessage};

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn by_id(&self) -> ById {
        ById
    }

    async fn me(&self, context: &Context<'_>) -> FieldResult<User> {
        Ok(context.cx().user().await?)
    }

    async fn conversations(&self, context: &Context<'_>) -> FieldResult<Vec<Conversation>> {
        Ok(Conversation::all(context.cx().surreal(), &context.cx().user().await?).await?)
    }

    async fn conversation_direct(
        &self,
        context: &Context<'_>,
        recipient: ID,
    ) -> FieldResult<Conversation> {
        Ok(Conversation(
            context.cx().ref_user()?,
            MessageRecipient::User(Ref::new(&recipient)),
        ))
    }

    async fn guilds(&self, context: &Context<'_>) -> FieldResult<Vec<Guild>> {
        #[derive(Deserialize)]
        struct Memer {
            guild: Guild,
        }
        let uid_id = context.cx().ref_user()?;
        let uid = uid_id.id();
        let query = format!(
            r#"
                SELECT guild FROM member WHERE user = user:{uid} FETCH guild.*
            "#
        );
        let memers: Vec<Memer> = context.cx().surreal().query(query).await?.take(0)?;
        Ok(memers.into_iter().map(|memer| memer.guild).collect())
    }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    async fn add_friend(&self, context: &Context<'_>, other: String) -> FieldResult<Option<User>> {
        let tag = parse_tag(&other).ok_or_else(|| anyhow::anyhow!("invalid friend tag"))?;
        let user = User::find_tag(context.cx().surreal(), &tag).await?;
        if user.is_none() {
            return Ok(None);
        }
        Ok(Some(
            context
                .cx()
                .user()
                .await?
                .add_friend(context.cx().surreal(), user.unwrap())
                .await?,
        ))
    }

    async fn set_theme(&self, context: &Context<'_>, theme: Theme) -> FieldResult<User> {
        let mut user = context.cx().user().await?;
        user.theme = theme;
        Ok(user.save(&SURREAL).await?)
    }

    async fn set_avatar(&self, context: &Context<'_>, avatar: Upload) -> FieldResult<User> {
        let f = avatar.value(context)?;

        context
            .storage()
            .write()
            .await
            .put_avatar_graphql(
                context.cx().ref_user()?.id().to_owned(),
                crate::storage::AvatarKind::U,
                crate::storage::AvatarFiletype::Static,
                f,
            )
            .await?;

        Ok(context.cx().user().await?)
    }

    async fn send_message(
        &self,
        context: &Context<'_>,
        message: MessageInit,
    ) -> FieldResult<Message> {
        Ok(context
            .cx()
            .user()
            .await?
            .send_message(context.cx().surreal(), context.relay(), message)
            .await?)
    }

    async fn create_guild(&self, context: &Context<'_>, guild: GuildInit) -> FieldResult<Guild> {
        let user = context.cx().user().await?;

        Guild::create(context.cx().surreal(), &user, guild).await
    }

    async fn set_status(&self, context: &Context<'_>, status: Status) -> FieldResult<User> {
        let mut user = context.cx().user().await?;
        user.status = status;
        Ok(user.save(context.cx().surreal()).await?)
    }

    async fn manage_message(
        &self,
        cx: &Context<'_>,
        message: ID,
    ) -> FieldResult<Option<ManageMessage>> {
        let m: Option<_> = SURREAL
            .select(message.as_str().parse::<RecordId>()?.0)
            .await?;
        Ok(if let Some(m) = m {
            Some(ManageMessage::new(cx.cx().user().await?, m))
        } else {
            None
        })
    }
}

pub struct SubscriptionRoot;

#[Subscription]
impl SubscriptionRoot {
    async fn messages(&self, context: &Context<'_>) -> Result<impl Stream<Item = Message>> {
        let user = context.cx().ref_user()?;

        let messages_stream = context.relay().stream_sent_messages().await;

        Ok(messages_stream.filter(move |message| {
            future::ready(matches!(
                &message.recipient,
                MessageRecipient::User(ref recipient) if recipient.id() == user.id()
            ))
        }))
    }
}

pub type Schema = async_graphql::Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

pub fn schema() -> Schema {
    schema_builder().finish()
}

pub fn schema_builder() -> SchemaBuilder<QueryRoot, MutationRoot, SubscriptionRoot> {
    async_graphql::Schema::build(QueryRoot, MutationRoot, SubscriptionRoot)
        .extension(async_graphql::extensions::Logger)
}

lazy_static::lazy_static! {
    pub static ref SCHEMA: Schema = schema();
}
