#![allow(unused)]
use std::{
    any::{type_name, Any},
    borrow::Cow,
    fmt::Display,
    marker::PhantomData,
    str::FromStr, future::IntoFuture, pin::Pin,
};

use async_graphql::{
    InputObjectType, InputType, InputValueError, InputValueResult, ScalarType, ID,
};
use async_std::sync::RwLock;
use chrono::{DateTime, Duration, Utc};
use futures_util::{Future, future::{Ready, self}};
use serde::{de::DeserializeOwned, de::Error, Deserialize, Serialize};
use serde_json::Value;
use serde_with::{serde_as, DurationSeconds as DurateSeconds, TimestampMilliSeconds};
use surrealdb::sql::{thing, Id, Thing};
use tide::log::error;

use crate::{pubsub::Relay, storage::Storage};

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DurationSeconds(#[serde_as(as = "DurateSeconds<i64>")] pub Duration);

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Datetime(#[serde_as(as = "TimestampMilliSeconds<i64>")] pub DateTime<Utc>);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecordId(pub Thing);

impl<'de> Deserialize<'de> for RecordId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self(match Value::deserialize(deserializer)? {
            Value::String(s) => thing(&s).map_err(D::Error::custom)?,
            Value::Object(o) => Thing {
                tb: o.get("tb").unwrap().as_str().unwrap().to_owned(),
                id: serde_json::from_value(o.get("id").unwrap().to_owned())
                    .map_err(D::Error::custom)?,
            },
            _ => {
                return Err(D::Error::custom(
                    "Invalid RecordId (not String, not Object, something else ????)",
                ))
            }
        }))
    }
}
impl Serialize for RecordId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl RecordId {
    pub fn new(table: &str, id: &str) -> Self {
        Self(Thing {
            tb: table.to_owned(),
            id: Id::String(id.to_owned()),
        })
    }
    pub fn id(&self) -> String {
        self.0.id.to_raw()
    }

    pub async fn fetch<T: DeserializeOwned + Send + Sync>(
        &self,
        surreal: &crate::Surreal,
    ) -> surrealdb::Result<T> {
        surreal.select(self.0.clone()).await
    }
}

impl TryFrom<ID> for RecordId {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: ID) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}

impl Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RecordId {
    type Err = surrealdb::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(thing(s)?))
    }
}

pub trait BooleanWhy {
    fn why<T>(self, val1: T, val2: T) -> T
    where
        Self: Sized;

    fn why_fn<T, F1: Fn() -> T, F2: Fn() -> T>(self, val1: F1, val2: F2) -> T
    where
        Self: Sized;
}

impl BooleanWhy for bool {
    fn why<T>(self, val1: T, val2: T) -> T {
        if self {
            val1
        } else {
            val2
        }
    }
    fn why_fn<T, F1: Fn() -> T, F2: Fn() -> T>(self, val1: F1, val2: F2) -> T {
        if self {
            val1()
        } else {
            val2()
        }
    }
}

pub trait Referrable {
    const TABLE: &'static str;
}

pub trait ReferrableWithId: Referrable {
    type Id: 'static + Clone + Eq + Send + Sync = String;
    fn id(&self) -> &Self::Id;
}

pub trait ReferrableExt: ReferrableWithId {
    fn refer(&self) -> Ref<Self>;
    fn record_id(&self) -> RecordId;

    async fn save(&self, surreal: &crate::Surreal) -> surrealdb::Result<Self>
    where
        Self: Serialize + Sized + DeserializeOwned + Send + Sync,
        Self::Id: AsRef<str>;

    fn gql_id_just(&self) -> ID;

    fn gql_id(&self) -> ID;
}

impl<R: ReferrableWithId<Id: AsRef<str> + for<'s> From<&'s str>>> ReferrableExt for R {
    default fn refer(&self) -> Ref<Self> {
        Ref::new_id(self.id().as_ref().into())
    }

    default fn record_id(&self) -> RecordId {
        RecordId((Self::TABLE.to_owned(), self.id().as_ref().to_owned()).into())
    }

    async fn save(&self, surreal: &crate::Surreal) -> surrealdb::Result<Self>
    where
        Self: Serialize + Sized + DeserializeOwned + Send + Sync,
    {
        surreal.update(self.record_id().0).content(self).await
    }

    // difference from gql_id: gives only the ID, without the table
    default fn gql_id_just(&self) -> ID {
        ID::from(self.id().as_ref())
    }

    // difference from just: includes the table
    default fn gql_id(&self) -> ID {
        ID(format!("{}:{}", Self::TABLE, self.id().as_ref()))
    }
}

#[derive(Debug, Clone)]
pub struct Ref<T: ReferrableWithId + ?Sized> {
    id: T::Id,
    phantom: PhantomData<T>,
}

impl<T: ReferrableWithId + ?Sized> PartialEq for Ref<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: ReferrableWithId + ?Sized> Eq for Ref<T> {}

impl<T: ReferrableWithId + ?Sized> Ref<T> {
    pub fn new(id: &str) -> Self
    where
        T: ReferrableWithId<Id = String>,
    {
        Self::_new(id.trim_start_matches(&format!("{}:", T::TABLE)).to_owned())
    }

    fn _new(id: <T as ReferrableWithId>::Id) -> Self {
        Self {
            id,
            phantom: PhantomData,
        }
    }

    pub fn new_owned(id: String) -> Self
    where
        T: ReferrableWithId<Id = String>,
    {
        Self::_new(id.trim_start_matches(&format!("{}:", T::TABLE)).to_owned())
    }

    pub fn new_id(id: T::Id) -> Self {
        Self::_new(id)
    }

    #[doc(hidden)]
    #[deprecated(note = "use new_owned instead")]
    pub fn _new_owned(id: String) -> Self
    where
        T: ReferrableWithId<Id = String>,
    {
        Self::new_owned(id)
    }

    pub fn record_id(&self) -> RecordId
    where
        T: ReferrableWithId<Id: Into<Id>>,
    {
        RecordId(Thing::from((T::TABLE.to_owned(), self.id.clone().into())))
    }

    pub fn just_id(&self) -> &T::Id {
        &self.id
    }

    pub fn into_id(self) -> T::Id {
        self.id
    }

    pub fn id_str(&self) -> &str
    where
        T: ReferrableWithId<Id = Id>,
    {
        unwrap_id_str(&self.id).unwrap()
    }

    pub fn id(&self) -> &str
    where
        T: ReferrableWithId<Id: AsRef<str>>,
    {
        &self.id.as_ref()
    }

    pub fn gql_id(&self) -> async_graphql::ID
    where
        T::Id: Display,
    {
        async_graphql::ID::from(format!("{}:{}", T::TABLE, self.id))
    }
}

impl<T: ReferrableWithId<Id = String> + ?Sized> TryFrom<RecordId> for Ref<T> {
    type Error = anyhow::Error;
    fn try_from(value: RecordId) -> Result<Self, Self::Error> {
        if value.0.tb != T::TABLE {
            return Err(anyhow::anyhow!("invalid table"));
        }

        Ok(Self::new_owned(value.id()))
    }
}

impl<T: ReferrableWithId<Id: Into<Id>> + DeserializeOwned + Sync + Send> Ref<T> {
    pub async fn fetch(&self, surreal: &crate::Surreal) -> surrealdb::Result<T> {
        surreal.select(self.record_id().0).await
    }
}

impl<T: ReferrableWithId<Id: Into<Id>>> Serialize for Ref<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.record_id().serialize(serializer)
    }
}
impl<'de, T: ReferrableWithId<Id = String>> Deserialize<'de> for Ref<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let rid = RecordId::deserialize(deserializer)
            .inspect_err(|e| error!("ref deserializement error on record id: {e}"))?;
        if rid.0.tb != T::TABLE {
            return Err(D::Error::custom("invalid table"));
        }
        Ok(Self::new_owned(rid.id()))
    }
}

impl<T: ReferrableWithId<Id = String> + ?Sized + Any + Send + Sync> InputType for Ref<T> {
    type RawValueType = ID;
    fn as_raw_value(&self) -> Option<&Self::RawValueType> {
        None
    }
    fn parse(value: Option<async_graphql::Value>) -> async_graphql::InputValueResult<Self> {
        let id =
            <ID as ScalarType>::parse(value.ok_or_else(|| InputValueError::custom("no value"))?)
                .map_err(InputValueError::propagate)?;
        Ok(Ref::new(id.as_str()))
    }
    fn to_value(&self) -> async_graphql::Value {
        async_graphql::Value::String(self.id().to_owned())
    }
    fn type_name() -> std::borrow::Cow<'static, str> {
        Cow::Borrowed("ID")
    }
    fn create_type_info(registry: &mut async_graphql::registry::Registry) -> String {
        Self::qualified_type_name()
    }
    fn federation_fields() -> Option<String> {
        None
    }
    fn qualified_type_name() -> String {
        Self::type_name().into_owned()
    }
}

pub(crate) macro referrable {
    ($thing:path = $tb:literal) => {
        impl crate::util::Referrable for $thing {
            const TABLE: &'static str = $tb;
        }
    },
    ($thing:path = $tb:literal .$id:ident) => {
        impl crate::util::Referrable for $thing {
            const TABLE: &'static str = $tb;
        }
        impl crate::util::ReferrableWithId for $thing {
            fn id(&self) -> &str {
                &self.$id
            }
        }
    },
    ($thing:path = $tb:literal .$id:ident: Thing) => {
        impl crate::util::Referrable for $thing {
            const TABLE: &'static str = $tb;
        }
        impl crate::util::ReferrableWithId for $thing {
            type Id = String;
            fn id(&self) -> &String {
                crate::util::unwrap_id_str(&self.$id.id).unwrap()
            }
        }
    },
    ($thing:path = $tb:literal .$id:ident: Option<Thing>) => {
        impl crate::util::Referrable for $thing {
            const TABLE: &'static str = $tb;
        }
        impl crate::util::ReferrableWithId for $thing {
            type Id = String;
            fn id(&self) -> &String {
                crate::util::unwrap_id_str(&self.$id.as_ref().unwrap().id).unwrap()
            }
        }
    }
}

pub fn unwrap_id_str(id: &Id) -> Option<&String> {
    match id {
        surrealdb::sql::Id::String(ref s) => Some(s),
        _ => None,
    }
}

pub trait Cx<'a> {
    fn cx(&self) -> &'a crate::http::State;
    fn relay(&self) -> &'a Relay;
    fn storage(&self) -> &'a RwLock<Storage>;
}

impl<'a> Cx<'a> for async_graphql::Context<'a> {
    fn cx(&self) -> &'a crate::http::State {
        self.data_unchecked()
    }
    fn relay(&self) -> &'a Relay {
        self.data_unchecked::<std::sync::Arc<Relay>>()
    }
    fn storage(&self) -> &'a RwLock<Storage> {
        self.data_unchecked::<std::sync::Arc<RwLock<Storage>>>()
    }
}
