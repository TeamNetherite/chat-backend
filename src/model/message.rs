use crate::util::{referrable, RecordId, Ref, ReferrableExt};
use async_graphql::{
    connection::{query, Connection, Edge, EmptyFields},
    *,
};
use derive_more::{IsVariant, Unwrap};
use itertools::Itertools;
use surrealdb::sql::{Datetime, Thing};
use tide::log::{debug, info};

use super::{guild::TextableChannel, user::User};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub id: Thing,
    pub author: Ref<User>,
    pub recipient: MessageRecipient,
    pub created_at: Datetime,
    pub content: String,
    #[serde(default, with = "magic")]
    pub magic: Magic,
    #[serde(default)]
    pub reference: Option<Ref<Message>>,
}

referrable!(Message = "message" .id: Thing);

impl Message {
    // linebreaks, tabs, CR, nbsp, zwsp, etc. which could change content
    const SANITIZE: [char; 10] = [
        // linebreaks, tabs, cr
        '\n', '\t', '\r', '\u{0085}', // scary shit
        '\0',       // weird spaces
        '\u{00A0}', '\u{200B}', '\u{FEFF}', '\u{2028}', '\u{2029}',
    ];

    pub async fn create(
        surreal: &crate::Surreal,
        User { id: author, .. }: &User,
        init: MessageInit,
    ) -> tide::Result<Self> {
        let author = author.to_raw();
        let recipient = init.recipient;
        let recipient_json = serde_json::to_string(&recipient)?;
        let reference = init.reference;
        let reference_json = reference
            .map(|r| serde_json::to_string(&r))
            .unwrap_or_else(|| Ok(String::from("null")))?;
        let content: String = init
            .content
            .chars()
            .filter(|cr| !Self::SANITIZE.contains(cr))
            .flat_map(|v| {
                if v == '\\' {
                    vec!['\\', '\\', '\\', '\\']
                } else {
                    vec![v]
                }
            })
            .collect();
        let query = format!(
            r#"
            CREATE message CONTENT {{
                author: "{author}",
                recipient: {recipient_json},
                magic: 0,
                content: "{content}",
                created_at: time::now(),
                reference: {reference_json}
            }};
            "#
        );
        Ok(Option::unwrap(
            surreal.query(unindent::unindent(&query)).await?.take(0)?,
        ))
    }
}

bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Debug, Clone, Hash, Copy, Serialize, Deserialize, Default)]
    pub struct Magic: u32 {
        const INVITE = 0b00000001;
    }
}

mod magic {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::Magic;

    pub fn serialize<S>(v: &Magic, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(v.bits())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Magic, D::Error>
    where
        D: Deserializer<'de>,
    {
        u32::deserialize(deserializer)
            .map(Magic::from_bits)
            .map(Option::unwrap)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, IsVariant, Unwrap)]
#[serde(tag = "kind", content = "id")]
pub enum MessageRecipient {
    User(Ref<User>),
    Channel(Ref<TextableChannel>),
}

#[derive(Enum, Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum MessageRecipientInKind {
    User,
    Channel,
}

#[derive(Debug, Clone, InputObject, Serialize, Deserialize)]
pub struct MessageRecipientIn {
    #[graphql(name = "type")]
    pub kind: MessageRecipientInKind,
    pub id: ID,
}

impl MessageRecipient {
    pub fn record_id(&self) -> RecordId {
        match self {
            Self::User(user) => user.record_id(),
            Self::Channel(channel) => channel.record_id(),
        }
    }
    pub fn gql_id(&self) -> ID {
        match self {
            Self::User(user) => user.gql_id(),
            Self::Channel(channel) => channel.gql_id(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, InputObject)]
pub struct MessageInit {
    pub recipient: MessageRecipientIn,
    pub content: String,
    pub reference: Option<Ref<Message>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation(pub Ref<User>, pub MessageRecipient);

#[derive(Debug, Clone, SimpleObject)]
pub struct MessageEdge {
    pub cursor: i32,
    pub node: Message,
}

impl Conversation {
    pub async fn all_messages(&self, surreal: &crate::Surreal) -> tide::Result<Vec<Message>> {
        let query = format!(
            r#"
        SELECT * FROM message WHERE
            (
                author = {0} AND
                recipient.id = {1}
            ) OR
            (
                author = {1} AND
                recipient.id = {0}
            );
        "#,
            self.0.record_id(),
            self.1.record_id()
        );
        Ok(surreal.query(unindent::unindent(&query)).await?.take(0)?)
    }

    pub async fn messages_paginate(
        &self,
        surreal: &crate::Surreal,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<Connection<i64, Message, EmptyFields, EmptyFields>> {
        #[derive(Deserialize)]
        struct Counted {
            counted: i64,
        }

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
                            r#"SELECT count() as counted FROM message WHERE
                                (
                                    author = {0} AND
                                    recipient.id = {1}
                                ) OR
                                (
                                    author = {1} AND
                                    recipient.id = {0}
                                ) GROUP BY counted"#,
                            self.0.record_id(),
                            self.1.record_id()
                        ))
                        .await?
                        .take(0)?,
                    Counted { counted: 0 },
                );
                info!(
                    "count for {} <-> {}: {count}",
                    self.0.record_id(),
                    self.1.record_id()
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
                let before_surreal = (end > 0)
                    .then(|| format!("LIMIT BY {end}"))
                    .unwrap_or_default();
                let after_surreal = format!("START AT {start}");
                let query = format!(
                    r#"
                            SELECT * FROM message WHERE
                                (
                                    author = {0} AND
                                    recipient.id = {1}
                                ) OR
                                (
                                    author = {1} AND
                                    recipient.id = {0}
                                ) ORDER BY created_at {before_surreal} {after_surreal};
                        "#,
                    self.0.record_id(),
                    self.1.record_id()
                );
                debug!("{}", query);
                let messages: Vec<Message> =
                    surreal.query(unindent::unindent(&query)).await?.take(0)?;
                let mut messages = messages.into_iter().map(Some).collect::<Vec<_>>();

                let mut connection = Connection::new(start > 0, end < count);
                connection.edges.extend(
                    (start..end)
                        .enumerate()
                        .map(|(i, n)| Edge::new(n, messages.get_mut(i).unwrap().take().unwrap())),
                );
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }

    pub async fn all(surreal: &crate::Surreal, user: &User) -> tide::Result<Vec<Self>> {
        #[derive(Deserialize, Debug)]
        struct Just {
            author: Ref<User>,
            recipient: MessageRecipient,
        }

        let query = format!(
            r#"
                SELECT author, recipient FROM message WHERE
                    author = {0} OR
                    recipient.id = {0};
                "#,
            &user.id
        );

        let messages: Vec<Just> = surreal.query(unindent::unindent(&query)).await?.take(0)?;

        let groups = messages.into_iter().group_by(|a| a.author.clone());

        let friends = user
            .get_friends(surreal)
            .await?
            .into_iter()
            .map(|friend| Conversation(user.refer(), MessageRecipient::User(friend.refer())));

        let convos = groups
            .into_iter()
            .filter_map(|(author, mut convo)| Some(Conversation(author, convo.next()?.recipient)))
            .chain(friends)
            .unique_by(|Conversation(_, a)| a.record_id().id());

        let mut convos: Vec<_> = convos.collect();
        convos.retain(|a| a.1.is_user() && a.1.record_id().0 != user.id);

        Ok(convos)
    }
}
