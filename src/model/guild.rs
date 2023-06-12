use anyhow::anyhow;
use async_graphql::{
    connection::{query, Connection, Edge, EmptyFields},
    *,
};
use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;
use tide::log::info;

use crate::util::{referrable, unwrap_id_str, Ref, Referrable, ReferrableExt, ReferrableWithId};

use super::user::User;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Guild {
    pub id: Thing,
    pub name: String,
    #[serde(default)]
    pub join_constraint: JoinConstraint,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, Enum, PartialEq, Eq, Default)]
pub enum JoinConstraint {
    #[default]
    None = 0,
    VerifiedEmail = 1,
    // 10 minutes
    JustRegistered = 2,
    Phone = 3,
}

#[derive(Deserialize, Serialize, Debug, Clone, InputObject)]
pub struct GuildInit {
    pub name: String,
}

referrable!(Guild = "guild" .id: Thing);

impl Guild {
    pub async fn fetch_roles(&self, surreal: &crate::Surreal) -> surrealdb::Result<Vec<Role>> {
        let id = &self.id;
        surreal
            .query(format!("SELECT * FROM role WHERE guild = {id}"))
            .await?
            .take(0)
    }

    pub async fn create(
        surreal: &crate::Surreal,
        user: &User,
        GuildInit { name }: GuildInit,
    ) -> async_graphql::Result<Self> {
        let query = format!(
            r#"
                CREATE guild SET name = $name
            "#
        );

        let tag = user.tag_fmt();

        info!("query to create a guild for {tag}: {query}");

        let guild: Option<Guild> = surreal
            .query(query)
            .bind(("name", name.as_str()))
            .await?
            .take(0)?;
        let guild = guild.ok_or_else(|| anyhow!("no guild"))?;

        Member::create(surreal, user, &guild).await?;

        Ok(guild)
    }

    pub async fn members_paginate(
        &self,
        surreal: &crate::Surreal,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<Connection<i64, Member, EmptyFields, EmptyFields>> {
        #[derive(Deserialize)]
        struct Counted {
            counted: i64,
        }

        let gid = &self.id;

        query(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                let mut start = after.map(|a| a + 1).unwrap_or(0);
                let Counted { counted: count }: Counted = Option::unwrap_or(
                    surreal
                        .query(format!(
                            r#"SELECT count() as counted FROM member WHERE guild = {gid} GROUP BY counted"#,
                        ))
                        .await?
                        .take(0)?,
                    Counted { counted: 0 },
                );
                let mut end = before.unwrap_or(count);
                if let Some(first) = first {
                    end = (start + first as i64).min(end)
                }
                if let Some(last) = last {
                    start = if last as i64 > end - start && end < count {
                        end
                    } else {
                        (end - last as i64).max(0)
                    };
                }
                let before_surreal = (end > 0).then(|| format!("LIMIT BY {end}")).unwrap_or_default();
                let after_surreal = format!("START AT {start}");

                let query = format!(r#"
                SELECT * FROM member WHERE guild = {gid} {before_surreal} {after_surreal}
                "#);
 
                let members: Vec<Member> =
                    surreal.query(unindent::unindent(&query)).await?.take(0)?;
                let mut members = members.into_iter().map(Some).collect::<Vec<_>>();

                let mut connection = Connection::new(start > 0, end < count);
                connection.edges.extend(
                    (start..end)
                        .enumerate()
                        .map(|(i, n)| Edge::new(n, members.get_mut(i).unwrap().take().unwrap())),
                );
                Ok::<_, async_graphql::Error>(connection)
            }
        )
        .await
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Member {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,
    pub guild: Ref<Guild>,
    pub nickname: Option<String>,
    pub user: Ref<User>,
    #[serde(default)]
    pub roles: Vec<Ref<Role>>,
}

referrable!(Member = "member" .id: Option<Thing>);

impl Member {
    pub async fn create(
        surreal: &crate::Surreal,
        user: &User,
        guild: &Guild,
    ) -> surrealdb::Result<Self> {
        let init = Member {
            id: None,
            guild: guild.refer(),
            nickname: None,
            user: user.refer(),
            roles: vec![]
        };
        surreal.create(Self::TABLE).content(init).await
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Role {
    pub id: Thing,
    pub name: String,
    pub color: u32,
    pub permissions: Vec<Permission>,
    pub guild: Ref<Guild>,
}

referrable!(Role = "role" .id: Thing);

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PermissionOverride {
    object: PermissionOverridable,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum PermissionOverridable {
    Channel(Ref<Channel>),
    Category(Ref<Category>),
    FullGuild,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, Enum, PartialEq, Eq)]
/// The possible permissions of a user or role in a guild.
pub enum Permission {
    /// A user with this permission may remove another user from the guild.
    Kick,
    /// A user with this permission may ban another user from the guild.
    Ban,
    /// A user with this permission may time another user out from the guild.
    /// Read [Timeout](super::audit::Timeout) docs for more info on what a time out is.
    Timeout,
    /// A user with this permission may create invitation links to the guild.
    Invite,
    ManageRoles,
    ManageChannels,
    ManageMessages,
    ManageWebhooks,
    ManageEmojis,
    SendMessages,

    ManageServer,
    Administrator,
}

#[derive(Deserialize, Serialize, Debug, Clone, Union)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Channel {
    Text(TextChannel),
}

#[derive(Deserialize, Serialize, Debug, Clone, Interface)]
#[serde(tag = "kind")]
#[graphql(field(name = "identifier", type = "ID"), field(name = "name", type = "String"))]
pub enum TextableChannel {
    #[serde(rename = "text")]
    Normal(TextChannel),
}

referrable!(TextableChannel = "channel");
referrable!(Channel = "channel");
referrable!(TextChannel = "channel" .id: Thing);

impl Channel {
    pub fn thing_id(&self) -> &Thing {
        match self {
            Self::Text(ref t) => &t.id,
        }
    }
}

impl TextableChannel {
    pub fn thing_id(&self) -> &Thing {
        match self {
            Self::Normal(ref t) => &t.id,
        }
    }
}

impl ReferrableWithId for Channel {
    fn id(&self) -> &String {
        unwrap_id_str(&self.thing_id().id).unwrap()
    }
}

impl ReferrableWithId for TextableChannel {
    fn id(&self) -> &String {
        unwrap_id_str(&self.thing_id().id).unwrap()
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct TextChannel {
    #[graphql(skip)]
    pub id: Thing,
    pub name: String,
    #[graphql(skip)]
    pub guild: Ref<Guild>,
}


#[derive(Deserialize, Serialize, Debug, Clone, Copy, derive_more::Display, Enum, PartialEq, Eq)]
pub enum ChannelKind {
    Text,
}

#[derive(Deserialize, Serialize, Debug, Clone, InputObject)]
pub struct ChannelInit {
    pub kind: ChannelKind,
    pub name: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Category {
    pub id: Thing,
    pub name: String,
    // one to one
    pub guild: Ref<Guild>,
    // one to many
    pub channels: Vec<Ref<Channel>>,
}

referrable!(Category = "category" .id: Thing);
