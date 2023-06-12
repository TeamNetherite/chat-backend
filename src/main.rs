#![feature(decl_macro)]
#![feature(result_option_inspect)]
#![feature(async_fn_in_trait)]
#![feature(return_position_impl_trait_in_trait)]
#![feature(associated_type_defaults)]
#![feature(associated_type_bounds)]
#![feature(default_free_fn)]
#![feature(negative_impls)]
#![feature(auto_traits)]
#![feature(specialization)]
use std::{env, str::FromStr};

use chrono::{Datelike, Utc};
use surrealdb::{engine::remote::ws, opt::auth::Root};
use tide::log::{info, warn, LevelFilter};

use crate::http::SURREAL;

mod auth;
mod graphql;
mod http;
mod jwt;
mod model;
mod pubsub;
mod storage;
mod util;

pub type Surreal = surrealdb::Surreal<ws::Client>;

enum LE {
    NoVar,
    InvalidLL(String),
}

static LOG_LEVEL_NAMES: [&str; 6] = ["OFF", "ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

#[async_std::main]
async fn main() -> tide::Result<()> {
    dotenv::dotenv()?;

    let cd_dir = env::var("NETHERITE_CHAT_CD").ok();
    if let Some(cd_dir) = cd_dir {
        env::set_current_dir(cd_dir)?;
    }

    let log_level_env = env::var("NETHERITE_CHAT_LOG_LEVEL");
    if let Some(level) = log_level_env
        .map_err(|_| LE::NoVar)
        .and_then(|var| LevelFilter::from_str(&var).map_err(|_| LE::InvalidLL(var)))
        .map(Some)
        .unwrap_or_else(|e| {
            if let LE::InvalidLL(var) = e {
                tide::log::with_level(LevelFilter::Info);
                warn!(
                    "guh the envvar value ({var}) is invalid. valid ones are: [{}]",
                    LOG_LEVEL_NAMES.join(", ")
                );
                None
            } else {
                Some(LevelFilter::Info)
            }
        })
    {
        tide::log::with_level(level)
    }

    let date = Utc::now();
    if date.month() == 5 && date.day() == 23 {
        info!("Happy birthday Remy_Clarke!");
    }

    SURREAL
        .connect::<ws::Ws>(env::var("NETHERITE_CHAT_SURREALDB_URL")?)
        .await?;
    SURREAL
        .signin(Root {
            username: "root",
            password: "root",
        })
        .await?;
    SURREAL.use_ns("netherite").use_db("chat").await?;
    http::run().await?;

    Ok(())
}
