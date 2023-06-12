use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;

use crate::util::{DurationSeconds, Datetime};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Timeout {
    pub user: Thing,
    pub duration: DurationSeconds,
    pub reason: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Kick {
    pub user: Thing,
    pub reason: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Ban {
    pub user: Thing,
    pub reason: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum AuditLogEntryType {
    Timeout(Timeout),
    Kick(Kick),
    Ban(Ban)
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct AuditLogEntry {
    pub entry_type: AuditLogEntryType,
    pub by: Thing,
    pub timestamp: Datetime,
}
