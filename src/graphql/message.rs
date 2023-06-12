use std::pin::Pin;

use async_graphql::connection::{Connection, EmptyFields};
use async_graphql::*;
use futures_util::Future;

use crate::http::SURREAL;
use crate::model::guild::TextableChannel;
use crate::model::message::{Conversation, Message, MessageRecipient};
use crate::model::user::User;
use crate::util::{Cx, ReferrableExt};

#[Object]
impl Message {
    async fn id(&self) -> ID {
        self.id.to_raw().into()
    }
    async fn author(&self, context: &Context<'_>) -> Result<User> {
        Ok(self.author.fetch(context.cx().surreal()).await?)
    }
    async fn content(&self) -> &str {
        &self.content
    }
    async fn recipient(&self) -> Result<MessageRecipient> {
        Ok(self.recipient.clone())
    }
    async fn created_at(&self) -> String {
        self.created_at.0.to_rfc3339()
    }

    async fn can_delete(&self, context: &Context<'_>) -> Result<bool> {
        Ok(context.cx().ref_user()? == self.author)
    }

    async fn reference(&self) -> Result<Option<Message>> {
        if let Some(ref reply) = self.reference {
            return Ok(Some(reply.fetch(&SURREAL).await?));
        }

        Ok(None)
    }
}

#[derive(Enum, Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MessageRecipientKind {
    User,
    Channel,
}

#[Object]
impl MessageRecipient {
    async fn kind(&self) -> MessageRecipientKind {
        match self {
            Self::User(_) => MessageRecipientKind::User,
            Self::Channel(_) => MessageRecipientKind::Channel,
        }
    }
    async fn as_user(&self, context: &Context<'_>) -> Result<Option<User>> {
        Ok(match self {
            Self::User(u) => Some(u.fetch(context.cx().surreal()).await?),
            _ => None,
        })
    }
    async fn as_channel(&self, context: &Context<'_>) -> Result<Option<TextableChannel>> {
        Ok(match self {
            Self::Channel(c) => Some(c.fetch(context.cx().surreal()).await?),
            _ => None,
        })
    }
}

#[Object]
impl Conversation {
    async fn messages(
        &self,
        context: &Context<'_>,
        first: Option<i32>,
        after: Option<String>,
        last: Option<i32>,
        before: Option<String>,
    ) -> Result<Connection<i64, Message, EmptyFields, EmptyFields>> {
        self.messages_paginate(context.cx().surreal(), after, before, first, last)
            .await
    }

    async fn get_all_messages(&self, context: &Context<'_>) -> Result<Vec<Message>> {
        Ok(self.all_messages(context.cx().surreal()).await?)
    }

    async fn author(&self, context: &Context<'_>) -> Result<User> {
        Ok(self.0.fetch(context.cx().surreal()).await?)
    }

    async fn recipient(&self) -> &MessageRecipient {
        &self.1
    }

    async fn id(&self) -> String {
        self.1.gql_id().to_string()
    }
}
