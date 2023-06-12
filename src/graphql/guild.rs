use crate::model::guild::*;
use crate::model::message::{Conversation, MessageRecipient};
use crate::model::user::User;
use crate::util::{unwrap_id_str, Cx, ReferrableExt, Ref, ReferrableWithId};
use async_graphql::*;
use async_graphql::connection::{Connection, EmptyFields};
use serde::Deserialize;

#[Object]
impl Role {
    async fn name(&self) -> &str {
        &self.name
    }
    async fn color(&self) -> i32 {
        self.color as i32
    }
    async fn permissions(&self) -> &[Permission] {
        &self.permissions
    }
}

#[Object]
impl Member {
    async fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref()
    }
    async fn roles(&self, cx: &Context<'_>) -> FieldResult<Vec<Role>> {
        #[derive(Deserialize)]
        struct Roles {
            roles: Vec<Role>
        }
        let roles: Option<Roles> = cx.cx().surreal().query("SELECT roles FROM $this FETCH roles.*").bind(("this", &self.id)).await?.take(0)?;
        Ok(roles.unwrap().roles)
    }
    async fn user(&self, cx: &Context<'_>) -> FieldResult<User> {
        Ok(self.user.fetch(cx.cx().surreal()).await?)
    }
}

#[Object]
impl Guild {
    async fn id(&self) -> ID {
        self.gql_id_just()
    }
    async fn name(&self) -> &str {
        &self.name
    }
    async fn roles(&self, cx: &Context<'_>) -> Result<Vec<Role>> {
        Ok(self.fetch_roles(cx.cx().surreal()).await?)
    }
    async fn members(
        &self,
        cx: &Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<Connection<i64, Member, EmptyFields, EmptyFields>> {
        self.members_paginate(cx.cx().surreal(), after, before, first, last).await
    }
    async fn channels(&self, cx: &Context<'_>) -> Result<Vec<Channel>> {
        let gid = unwrap_id_str(&self.id.id).unwrap();
        let query = format!(
            r#"
            SELECT * FROM channel WHERE guild = guild:{gid}
        "#
        );

        Ok(cx.cx().surreal().query(query).await?.take(0)?)
    }

    async fn create_channel(&self, cx: &Context<'_>, init: ChannelInit) -> Result<Channel> {
        let ChannelInit { name, kind } = init;
        let gid = unwrap_id_str(&self.id.id).unwrap();
        let query = format!(
            r#"
            CREATE channel CONTENT {{
                guild: {gid},
                name: $name,
                kind: '{kind}'
            }}
        "#
        );
        Ok(Option::unwrap(
            cx.cx()
                .surreal()
                .query(query)
                .bind(("name", name.as_str()))
                .await?
                .take(0)?,
        ))
    }

    async fn join_constraint(&self) -> JoinConstraint {
        self.join_constraint
    }
}

#[ComplexObject]
impl TextChannel {
    pub async fn identifier(&self) -> ID {
        <Self as ReferrableExt>::gql_id_just(self)
    }
    async fn guild(&self) -> ID {
        self.guild.gql_id()
    }
    async fn talk(&self, cx: &Context<'_>) -> Result<Conversation> {
        Ok(Conversation(cx.cx().ref_user()?, MessageRecipient::Channel(Ref::new(<Self as ReferrableWithId>::id(self).as_ref()))))
    }
}
