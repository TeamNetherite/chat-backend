use anyhow::anyhow;
use async_std::future::timeout;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation};
use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};
use tide::{http::mime::JSON, log::info, Body, Request, Response, StatusCode};
use crate::jwt::JwtAuthenticationDecoder;

use crate::{http::HttpState as State, model::user::User, util::{RecordId, BooleanWhy}};

#[derive(Serialize)]
struct Tokens {
    access: String,
    refresh: String,
}

#[derive(Deserialize)]
struct Cred {
    email: String,
    password: String,
}

pub async fn http_login(mut request: Request<State>) -> tide::Result<impl Into<Response>> {
    let credentials = request.body_json().await?;
    if let Some(tokens) = login(request.state(), credentials).await? {
        Ok(Response::builder(StatusCode::Ok)
            .body(Body::from_json(&tokens)?)
            .content_type(JSON))
    } else {
        Ok(Response::builder(StatusCode::BadRequest))
    }
}

pub async fn http_register(mut request: Request<State>) -> tide::Result<impl Into<Response>> {
    let data = request.body_json().await?;
    if let Some(tokens) = register(request.state(), data).await? {
        Ok(Response::builder(StatusCode::Ok)
            .body(Body::from_json(&tokens)?)
            .content_type(JSON))
    } else {
        Ok(Response::builder(StatusCode::BadRequest))
    }
}

pub async fn http_refresh(mut request: Request<State>) -> tide::Result {
    let refresh_token = request.body_string().await?;
    if let Some(tokens) = refresh(request.state(), &refresh_token).await? {
        Ok(Response::builder(StatusCode::Ok)
            .body(Body::from_json(&tokens)?)
            .content_type(JSON)
            .into())
    } else {
        Ok(Response::new(StatusCode::BadRequest))
    }
}

pub async fn http_isactive(request: Request<State>) -> tide::Result {
    #[derive(Deserialize)]
    struct Q {
        token: String,
    }
    let Q { token } = request.query()?;
    let activeness = is_active(request.state().surreal(), &token).await;
    let status = activeness.is_ok().why(StatusCode::Ok, StatusCode::BadRequest);
    let mut response = Response::builder(status);

    if activeness.is_ok() {
        response = response.body(Body::from_json(&activeness.unwrap())?).content_type(JSON);
    }

    Ok(response.build())
}

pub async fn is_active(surreal: &crate::Surreal, token: &str) -> tide::Result<bool> {
    let jwt = JwtKind::demake_independent(token)?;
    let jwt_db: Option<Jwt> = surreal.select(("jwt", &jwt.jti.id())).await.map_err(|e| tide::Error::new(StatusCode::InternalServerError, e))?;

    Ok(jwt_db.and_then(Jwt::check).is_some_and(|j| !j.expired()))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Claims {
    pub uid: RecordId,
}

#[derive(Clone, Debug)]
pub struct JwtToken {
    pub db: Jwt,
    pub claims: Claims_,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum JwtKind {
    Access,
    Refresh,
}

lazy_static::lazy_static! {
    static ref ACCESS: String = std::env::var("NETHERITE_CHAT_TIDY_ACCESS").unwrap();
    static ref REFRESH: String = std::env::var("NETHERITE_CHAT_TIDY_REFRESH").unwrap();
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Claims_ {
    #[serde(with = "jwt_numeric_date")]
    pub exp: DateTime<Utc>,
    #[serde(with = "jwt_numeric_date")]
    pub iat: DateTime<Utc>,
    #[serde(flatten)]
    pub claims: Claims,
    pub jti: RecordId,
    pub sub: JwtKind,
}
impl JwtKind {
    async fn make(&self, state: &State, claims: Claims) -> Result<String, anyhow::Error> {
        let iat = Utc::now();
        let jw: Jwt = state
            .surreal()
            .create("jwt")
            .content(Jwt {
                id: None,
                kind: *self,
                uid: claims.uid.clone(),
                issued_at: Datetime(iat),
                active: true,
            })
            .await?;
        let jti = jw.id.unwrap();

        let claims_real = Claims_ {
            exp: iat.checked_add_signed(self.expiry()).unwrap(),
            iat,
            jti: RecordId(jti),
            claims,
            sub: *self,
        };

        let key = self.key_enc();
        let h = Header::new(Algorithm::HS256);
        Ok(jsonwebtoken::encode(&h, &claims_real, &key)?)
    }

    fn demake(&self, token: &str) -> Result<Claims_, anyhow::Error> {
        let val = Validation::new(Algorithm::HS256);
        let TokenData { header: _, claims } = decode::<Claims_>(token, &self.key_dec(), &val)?;
        Ok(claims)
    }

    fn demake_independent(token: &str) -> Result<Claims_, anyhow::Error> {
        let mut val = Validation::new(Algorithm::HS256);
        val.validate_nbf = false;
        val.validate_exp = false;
        val.insecure_disable_signature_validation();
        let TokenData { header: _, claims } =
            decode::<Claims_>(token, &JwtKind::Access.key_dec(), &val)?;

        Ok(claims)
    }

    fn key(&self) -> &[u8] {
        match self {
            Self::Access => &*ACCESS,
            Self::Refresh => &*REFRESH,
        }
        .as_bytes()
    }

    fn key_enc(&self) -> EncodingKey {
        EncodingKey::from_secret(self.key())
    }

    fn key_dec(&self) -> DecodingKey {
        DecodingKey::from_secret(self.key())
    }

    fn expiry(&self) -> Duration {
        match self {
            Self::Access => Duration::minutes(10),
            Self::Refresh => Duration::minutes(60),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Jwt {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Thing>,
    kind: JwtKind,
    uid: RecordId,
    issued_at: Datetime,
    active: bool,
}

impl Jwt {
    pub fn check(self) -> Option<Self> {
        (!self.expired()).then_some(self)
    }

    pub fn expired(&self) -> bool {
        Utc::now()
            > self
                .issued_at
                .checked_add_signed(self.kind.expiry())
                .unwrap()
            || !self.active
    }
}

mod jwt_numeric_date {
    use chrono::{DateTime, NaiveDateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes an OffsetDateTime to a Unix timestamp (milliseconds since 1970/1/1T00:00:00T)
    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let timestamp = date.timestamp();
        serializer.serialize_i64(timestamp)
    }

    /// Attempts to deserialize an i64 and use as a Unix timestamp
    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(DateTime::from_utc(
            NaiveDateTime::from_timestamp_millis(i64::deserialize(deserializer)?)
                .ok_or_else(|| serde::de::Error::custom("invalid unix timestamp"))?,
            Utc,
        ))
    }
}

async fn make_jwts(state: &State, uid: RecordId) -> Result<Tokens, anyhow::Error> {
    let access = JwtKind::Access
        .make(state, Claims { uid: uid.clone() })
        .await?;
    let refresh = JwtKind::Refresh.make(state, Claims { uid }).await?;
    Ok(Tokens { access, refresh })
}

async fn login(
    state: &State,
    Cred { email, password }: Cred,
) -> Result<Option<Tokens>, tide::Error> {
    #[derive(Deserialize)]
    struct PasswordHash {
        id: Thing,
        password_hash: String,
    }
    let real_hash: Option<PasswordHash> = state
        .surreal()
        .query(format!(
            "select password_hash, id from user where email == \"{email}\";"
        ))
        .await?
        .take(0)?;
    if real_hash.is_none() {
        info!("No password for email {email}");
        return Ok(None);
    }
    let PasswordHash {
        password_hash: real_hash,
        id: uid,
    } = real_hash.unwrap();
    let is_real = bcrypt::verify(password, &real_hash)?;

    if is_real {
        return Ok(Some(make_jwts(state, RecordId(uid)).await?));
    }

    info!("Password does not match for {email}");

    Ok(None)
}

#[derive(serde::Deserialize)]
struct RegisterData {
    #[serde(flatten)]
    credentials: Cred,
    tag: String,
    display_name: String,
}

pub async fn make_tag(state: &State, tag: &str) -> Result<[u8; 4], surrealdb::Error> {
    #[derive(serde::Deserialize)]
    struct TagTag {
        tag: [i32; 4],
    }
    use rand::Rng;
    let reals: Vec<TagTag> = state
        .surreal()
        .query("select tag[1] from user where tag[0] == $real_tag;")
        .bind(("real_tag", tag))
        .await?
        .take(0)?;
    let mut real = [0u8; 4];
    let reals = reals
        .into_iter()
        .map(|TagTag { tag: [x, y, z, w] }| [x as u8, y as u8, z as u8, w as u8])
        .collect::<Vec<_>>();
    let mut rng = rand::thread_rng();
    loop {
        rng.fill(&mut real);
        if reals.contains(&real) {
            continue;
        }

        return Ok(real);
    }
}

const SALT_ROUNDS: u32 = 10;

async fn register(
    state: &State,
    RegisterData {
        credentials: Cred { email, password },
        tag,
        display_name,
    }: RegisterData,
) -> Result<Option<Tokens>, tide::Error> {
    let password_hash = bcrypt::hash(password.as_bytes(), SALT_ROUNDS)?;
    if !state
        .surreal()
        .query("SELECT * FROM user WHERE email == $real_email;")
        .bind(("real_email", &email))
        .await?
        .take::<Vec<User>>(0)?
        .is_empty()
    {
        info!("user with {email} tried to register, already exists.");
        return Ok(None);
    }
    let [x, y, z, w] = timeout(Duration::seconds(10).to_std()?, make_tag(state, &tag)).await??;
    let query = format!(
        r#"
            CREATE user SET
                email = {email},
                password_hash = {password_hash},
                tag = ['{tag}', [{x}, {y}, {z}, {w}]],
                display_name = {display_name};
        "#
    );
    let query = unindent::unindent(&query);
    info!("creating user {tag}#{x:x}{y:x}{z:x}{w:x} with email {email}: \n{query}");
    let user: Option<User> = state.surreal().query(query).await?.check()?.take(0)?;
    let user = user.ok_or_else(|| anyhow!("user no makey???"))?;

    Ok(Some(make_jwts(state, RecordId(user.id)).await?))
}

async fn refresh(state: &State, token: &str) -> Result<Option<Tokens>, tide::Error> {
    let claims = JwtKind::Refresh.demake(token)?;
    let jwt: Option<Jwt> = state.surreal().select(("jwt", &claims.jti.id())).await?;
    let jwt = jwt.ok_or_else(|| anyhow!("token no exist"))?;
    if let Some(mut jwt) = jwt.check() {
        if let JwtKind::Refresh = &jwt.kind {
            jwt.active = false;
            let uid = jwt.uid.clone();
            state
                .surreal()
                .update::<Option<Jwt>>(jwt.id.as_ref().unwrap().clone())
                .content(jwt).await?;
            return Ok(Some(make_jwts(state, uid).await?));
        }
    };

    Ok(None)
}

pub fn make_tide_authware() -> JwtAuthenticationDecoder<Claims_> {
    JwtAuthenticationDecoder::new(Validation::new(Algorithm::HS256), JwtKind::Access.key_dec())
}
