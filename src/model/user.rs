use crate::pubsub::Relay;
use anyhow::anyhow;
use async_graphql::Enum;
use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;
use tide::StatusCode;

use crate::util::{referrable, ReferrableExt};

use super::message::{Message, MessageInit};

pub type Tag = (String, [i32; 4]);

pub fn parse_tag(tag: &str) -> Option<Tag> {
    let (name, discrim) = tag.split_once('#')?;
    let discriminator = discrim
        .chars()
        .take(4)
        .map(|c| c.to_digit(16).map(|w| w as i32))
        .collect::<Vec<Option<i32>>>();
    let [x, y, z, w] = discriminator.as_slice() else { unreachable!() };

    Some((
        name.to_owned(),
        [
            *(x.as_ref()?),
            *(y.as_ref()?),
            *(z.as_ref()?),
            *(w.as_ref()?),
        ],
    ))
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct User {
    pub id: Thing,
    pub tag: Tag,
    pub display_name: String,
    pub email: String,
    pub password_hash: String,
    #[serde(default)]
    pub badges: Vec<Badge>,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub theme: Theme
}

#[derive(Clone, Copy, Deserialize, Serialize, Debug, Enum, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light
}

impl User {
    pub fn tag_fmt(&self) -> String {
        let [x, y, z, w] = self.tag.1;
        format!("{}#{x:x}{y:x}{z:x}{w:x}", self.tag.0)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Enum, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Online,
    Idle,
    DoNotDisturb,
    #[default]
    Offline,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, Enum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    Admin,
    Moderator,
}

referrable!(User = "user" .id: Thing);

impl User {
    pub async fn add_friend(&self, surreal: &crate::Surreal, other: User) -> tide::Result<Self> {
        if self
            .get_friends(surreal)
            .await?
            .iter()
            .any(|a| a.id == other.id)
        {
            return Err(tide::Error::new(
                StatusCode::Conflict,
                anyhow!("already friends"),
            ));
        }
        surreal
            .query(format!(
                "RELATE {}->friends->{} SET time.friended = time::now(), friend.request = true;",
                self.id, other.id
            ))
            .await?;
        Ok(other)
    }
    pub async fn get_friends(&self, surreal: &crate::Surreal) -> tide::Result<Vec<User>> {
        #[derive(serde::Deserialize)]
        struct Friends {
            friends_direct: Vec<User>,
            friends_back: Vec<User>,
        }

        let friends: Option<Friends> = surreal
            .query(format!(
                "select ->friends->user.* as friends_direct, <-friends<-user.* as friends_back from {};",
                self.id
            ))
            .await?
            .check()?
            .take(0)?;
        let friends = friends.unwrap();
        let Friends {
            mut friends_direct,
            friends_back,
        } = friends;
        friends_direct.extend(friends_back);
        friends_direct.retain(|a| a.id != self.id);

        Ok(friends_direct)
    }

    pub async fn find_tag(
        surreal: &crate::Surreal,
        (name, [x, y, z, w]): &Tag,
    ) -> tide::Result<Option<Self>> {
        let user: Option<Self> = surreal
            .query(format!(
                "SELECT * FROM user WHERE tag = ['{name}', [{x}, {y}, {z}, {w}]]"
            ))
            .await?
            .take(0)?;
        Ok(user)
    }

    pub async fn send_message(
        &self,
        surreal: &crate::Surreal,
        relay: &Relay,
        init: MessageInit,
    ) -> tide::Result<Message> {
        let message = Message::create(surreal, self, init).await?;

        relay.send_message(&message).await;

        Ok(message)
    }
}
