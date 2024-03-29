#![allow(unused)]
use anyhow::anyhow;
use async_trait::async_trait;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{de::DeserializeOwned, Serialize};
use std::marker::PhantomData;
use tide::{Middleware, Next, Request, Response, StatusCode};

use crate::{auth::is_active, http::HttpState};

pub fn jwtsign<Claims: Serialize + DeserializeOwned + Send + Sync + 'static>(
    claims: &Claims,
    key: &EncodingKey,
) -> Result<String, jsonwebtoken::errors::Error> {
    encode(&Header::default(), claims, key)
}

pub fn jwtsign_secret<Claims: Serialize + DeserializeOwned + Send + Sync + 'static>(
    claims: &Claims,
    key: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_base64_secret(key)?,
    )
}

pub fn jwtsign_with<Claims: Serialize + DeserializeOwned + Send + Sync + 'static>(
    header: &Header,
    claims: &Claims,
    key: &EncodingKey,
) -> Result<String, jsonwebtoken::errors::Error> {
    encode(header, claims, key)
}

pub struct JwtAuthenticationDecoder<Claims: DeserializeOwned + Send + Sync + 'static> {
    pub validation: Validation,
    pub key: DecodingKey,
    _claims: PhantomData<Claims>,
}

impl<Claims: DeserializeOwned + Send + Sync + 'static> JwtAuthenticationDecoder<Claims> {
    pub fn default(key: DecodingKey) -> Self {
        Self::new(Validation::default(), key)
    }

    pub fn new(validation: Validation, key: DecodingKey) -> Self {
        Self {
            validation,
            key,
            _claims: PhantomData::default(),
        }
    }
}

#[async_trait]
impl<Claims> Middleware<HttpState> for JwtAuthenticationDecoder<Claims>
where
    Claims: DeserializeOwned + Send + Sync + 'static,
{
    async fn handle(&self, mut req: Request<HttpState>, next: Next<'_, HttpState>) -> tide::Result {
        let header = req.header("Authorization");
        if header.is_none() {
            return Ok(next.run(req).await);
        }

        let values: Vec<_> = header.unwrap().into_iter().collect();

        if values.is_empty() {
            return Ok(next.run(req).await);
        }

        if values.len() > 1 {
            return Ok(Response::new(StatusCode::Unauthorized));
        }

        for value in values {
            let value = value.as_str();
            if !value.starts_with("Bearer") {
                continue;
            }

            let token = &value["Bearer ".len()..];
            println!("found authorization token: {token}");
            if is_active(req.state().surreal(), token).await? {
                let data = match decode::<Claims>(token, &self.key, &self.validation) {
                    Ok(c) => c,
                    Err(_) => {
                        return Err(tide::Error::new(StatusCode::Unauthorized, anyhow!("invalid token")));
                    }
                };

                req.set_ext(data.claims);
                break;
            } else {
                return Err(tide::Error::new(StatusCode::Unauthorized, anyhow!("token is not active")));
            }
        }

        Ok(next.run(req).await)
    }
}
