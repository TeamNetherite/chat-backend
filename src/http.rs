use crate::{pubsub::Relay, storage::Storage};
use anyhow::anyhow;
use async_graphql::{http::GraphiQLSource, Data};
use async_graphql_tide::*;
use async_std::sync::RwLock;
use serde::Deserialize;
use std::{env, sync::Arc};
use tide::{
    http::{headers::HeaderValue, mime},
    log::{error, info, LogMiddleware},
    security::{CorsMiddleware, Origin},
    Body, Request, Response, StatusCode,
};

use crate::{
    auth::{self, Claims_, JwtKind},
    graphql::schema_builder,
    model::user::User,
    util::Ref,
};

#[derive(Clone)]
pub struct HttpState {
    pub relay: Arc<Relay>,
    pub storage: Arc<RwLock<Storage>>,
}

impl HttpState {
    pub fn surreal(&self) -> &super::Surreal {
        &crate::SURREAL
    }
}

#[derive(Clone, Debug)]
pub struct State {
    pub token: Option<auth::JwtToken>,
}

impl State {
    pub fn surreal(&self) -> &super::Surreal {
        &crate::http::SURREAL
    }
    pub async fn user(&self) -> tide::Result<User> {
        let uid = self
            .token
            .as_ref()
            .map(|token| token.claims.claims.uid.clone());
        if let Some(uid) = uid {
            let user: Option<User> = self.surreal().select(uid.0).await?;
            return user.ok_or_else(|| {
                error!("user not authed (surreal)");
                tide::Error::new(
                    StatusCode::Unauthorized,
                    anyhow::anyhow!("not authenticated (surreal)"),
                )
            });
        }
        error!("user is not authorized");
        Err(tide::Error::new(
            StatusCode::Unauthorized,
            anyhow::anyhow!("not authenticated"),
        ))
    }

    pub fn ref_user(&self) -> tide::Result<Ref<User>> {
        let uid = self
            .token
            .as_ref()
            .map(|token| token.claims.claims.uid.id())
            .ok_or_else(|| {
                tide::Error::new(
                    StatusCode::Unauthorized,
                    anyhow::anyhow!("not authenticated"),
                )
            })?;
        Ok(Ref::new_owned(uid))
    }
}

async fn graphiql(_: Request<HttpState>) -> tide::Result<impl Into<Response>> {
    Ok(Response::builder(200)
        .body(Body::from_string(
            GraphiQLSource::build()
                .endpoint("/graphql")
                .subscription_endpoint("/graphql-subscription")
                .finish(),
        ))
        .content_type(mime::HTML))
}

async fn gql_subscrimb(request: Request<HttpState>) -> tide::Result {
    let endpoint = GraphQLSubscription::on_connection_init(
        async_graphql_tide::GraphQLSubscription::new(
            crate::graphql::schema_builder()
                .data(request.state().relay.clone())
                .data(request.state().storage.clone())
                .finish(),
        ),
        move |val| async move {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct I {
                access_token: String,
            }

            let result: Result<_, async_graphql::Error> = async move {
                let token = if val.is_object() {
                    Some(serde_json::from_value::<I>(val)?).map(|i| i.access_token)
                } else {
                    None
                };

                let claims = if let Some(token) = token {
                    info!("oh boy, found authorization token: {token}");
                    let y = crate::auth::make_tide_authware();
                    if crate::auth::is_active(&SURREAL, &token).await? {
                        let data = match jsonwebtoken::decode::<crate::auth::Claims_>(
                            &token,
                            &y.key,
                            &y.validation,
                        ) {
                            Ok(c) => c,
                            Err(_) => {
                                return Err(async_graphql::Error::new("invalid token"));
                            }
                        };

                        Some(data.claims)
                    } else {
                        return Err(async_graphql::Error::new("inactive token"));
                    }
                } else {
                    None
                };
                let token = if let Some(c) = claims {
                    if let JwtKind::Refresh = c.sub {
                        None
                    } else {
                        Some(make_jwt_token(&c, &SURREAL).await?)
                    }
                } else {
                    None
                };
                let state = State { token };
                let mut d = Data::default();
                d.insert(state);
                Ok(d)
            }
            .await;
            match result {
                Err(ref e) => {
                    error!("error: {e:?}");
                    result
                }
                _ => result,
            }
        },
    )
    .build::<HttpState>();

    tide::Endpoint::call(&endpoint, request).await
}

pub async fn make_jwt_token(
    claims: &Claims_,
    surreal: &super::Surreal,
) -> tide::Result<auth::JwtToken> {
    let db: Option<auth::Jwt> = surreal.select(claims.jti.0.clone()).await?;
    let db = db.unwrap();
    let db = db.check().ok_or_else(|| {
        tide::Error::new(
            StatusCode::Unauthorized,
            anyhow!("token expired, /auth/refresh it"),
        )
    })?;
    Ok(auth::JwtToken {
        claims: claims.clone(),
        db,
    })
}

async fn handle_gql(request: Request<HttpState>) -> tide::Result {
    let surreal = &SURREAL;
    let claims = request.ext::<Claims_>();
    let token: tide::Result<_> = async move {
        if let Some(c) = claims {
            if let JwtKind::Refresh = c.sub {
                return Ok(None);
            }
            Ok(Some(make_jwt_token(c, surreal).await?))
        } else {
            Ok(None)
        }
    }
    .await;
    let state = State { token: token? };
    let schema = schema_builder()
        .data(state)
        .data(request.state().relay.clone())
        .data(request.state().storage.clone())
        .finish();
    let req = receive_request(request).await?;
    let response = schema.execute(req).await;
    let result = async_graphql_tide::respond(response);
    result.inspect_err(|e| error!("{e}"))
}

pub static SURREAL: crate::Surreal = crate::Surreal::init();

pub(super) async fn run() -> tide::Result<()> {
    let relay = Arc::new(Relay::new());
    let storage = Arc::new(RwLock::new(Storage::new()));
    let mut tide = tide::with_state(HttpState {
        relay,
        storage: storage.clone(),
    });
    tide.with(LogMiddleware::new());

    let s = storage.read().await;
    s.init_fs().await?;
    s.tide(&mut tide)?;
    drop(s);

    let cors = CorsMiddleware::new()
        .allow_methods("GET, POST, OPTIONS".parse::<HeaderValue>().unwrap())
        .allow_origin(Origin::from("*"))
        .allow_credentials(true);

    tide.with(cors);

    tide.at("/graphql")
        .with(auth::make_tide_authware())
        .post(handle_gql);
    tide.at("/graphiql")
        .with(auth::make_tide_authware())
        .get(graphiql);
    tide.at("/graphql-subscription")
        .with(auth::make_tide_authware())
        .get(gql_subscrimb);

    tide.at("/auth/login").post(auth::http_login);
    tide.at("/auth/register").post(auth::http_register);
    tide.at("/auth/refresh").post(auth::http_refresh);
    tide.at("/auth/isactive").get(auth::http_isactive);

    tide.listen(env::var("NETHERITE_CHAT_HTTP_URL")?).await?;

    Ok(())
}
