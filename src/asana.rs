use std::{
    collections::HashMap,
    io::{self, Write},
};

use anyhow::Context;
use chrono::{DateTime, Duration, Local};
use oauth2::{reqwest::async_http_client, TokenResponse};
use reqwest::{StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

const ASANA_API_BASE_URL: &str = "https://app.asana.com/api/1.0/";
const ASANA_OAUTH_AUTHORIZATION_URL: &str = "https://app.asana.com/-/oauth_authorize";
const ASANA_OAUTH_TOKEN_URL: &str = "https://app.asana.com/-/oauth_token";
const ASANA_OAUTH_LOCAL_REDIRECT_URI: &str = "urn:ietf:wg:oauth:2.0:oob";

const ASANA_APP_CLIENT_ID: &str = "1206215514588292";
const ASANA_APP_CLIENT_SECRET: &str = "8c7ea1c603de8462a3ba24f827ff1658";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Credentials {
    access_token: String,
    refresh_token: Option<String>,
}

#[must_use]
pub async fn execute_authorization_flow() -> anyhow::Result<Credentials> {
    log::debug!("Setting up OAuth client and authorization request...");
    let oauth_client = oauth2::basic::BasicClient::new(
        oauth2::ClientId::new(ASANA_APP_CLIENT_ID.to_string()),
        Some(oauth2::ClientSecret::new(
            ASANA_APP_CLIENT_SECRET.to_string(),
        )),
        oauth2::AuthUrl::new(ASANA_OAUTH_AUTHORIZATION_URL.to_string())?,
        Some(oauth2::TokenUrl::new(ASANA_OAUTH_TOKEN_URL.to_string())?),
    )
    .set_redirect_uri(oauth2::RedirectUrl::new(
        ASANA_OAUTH_LOCAL_REDIRECT_URI.to_string(),
    )?);
    let (pkce_challenge, pkce_verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
    let (auth_url, _) = oauth_client
        .authorize_url(oauth2::CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge)
        .url();

    log::info!("Opening browser to authorization URL...");
    println!("Opening your browser and sending you to {auth_url}...");
    open::that(auth_url.to_string()).context("could not open authorization URL in the browser")?;

    log::info!("Waiting for user to provide the authorization code...");
    print!("Once you're done, come back here and post the code you got: ");
    io::stdout().flush().context("could not flush stdout")?;
    let mut auth_code = String::new();
    io::stdin()
        .read_line(&mut auth_code)
        .context("could not read authorization code from stdin")?;

    log::info!("Exchanging authorization code for an access token...");
    let token = oauth_client
        .exchange_code(oauth2::AuthorizationCode::new(auth_code.trim().to_string()))
        .set_pkce_verifier(pkce_verifier)
        .request_async(async_http_client)
        .await
        .context("could not exchange authorization code for an access token")?;
    let credentials = Credentials {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token
            .refresh_token()
            .map(|token| token.secret().to_string()),
    };

    Ok(credentials)
}

#[must_use]
pub async fn refresh_authorization(
    refresh_token: &oauth2::RefreshToken,
) -> anyhow::Result<Credentials> {
    log::debug!("Setting up OAuth client...");
    let oauth_client = oauth2::basic::BasicClient::new(
        oauth2::ClientId::new(ASANA_APP_CLIENT_ID.to_string()),
        Some(oauth2::ClientSecret::new(
            ASANA_APP_CLIENT_SECRET.to_string(),
        )),
        oauth2::AuthUrl::new(ASANA_OAUTH_AUTHORIZATION_URL.to_string())?,
        Some(oauth2::TokenUrl::new(ASANA_OAUTH_TOKEN_URL.to_string())?),
    )
    .set_redirect_uri(oauth2::RedirectUrl::new(
        ASANA_OAUTH_LOCAL_REDIRECT_URI.to_string(),
    )?);

    let token = oauth_client
        .exchange_refresh_token(refresh_token)
        .request_async(async_http_client)
        .await
        .context("could not exchange refresh token for an access token")?;
    let credentials = Credentials {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token
            .refresh_token()
            .map(|token| token.secret().to_string()),
    };

    Ok(credentials)
}

pub trait DataRequest<'a> {
    type RequestData;
    type ResponseData: Serialize + DeserializeOwned;

    fn endpoint(request_data: Self::RequestData, base_url: &Url) -> Url;
    fn fields() -> &'static [&'static str];
    fn other_params() -> HashMap<String, String> {
        HashMap::default()
    }
}

#[derive(Deserialize, Serialize)]
struct DataResponse<D> {
    data: D,
}

#[derive(Debug, Error)]
enum ClientError {
    #[error("unable to refresh token")]
    UnableToRefreshToken,
}

pub struct Client {
    base_url: String,
    credentials: Credentials,
    inner_client: reqwest::Client,

    last_refresh_attempt: Option<DateTime<Local>>,
}

impl Client {
    fn construct_inner_client() -> anyhow::Result<reqwest::Client> {
        reqwest::ClientBuilder::new()
            .connect_timeout(Duration::seconds(5).to_std()?)
            .timeout(Duration::seconds(10).to_std()?)
            .build()
            .context("could not build Asana client")
    }

    pub async fn new() -> anyhow::Result<Client> {
        let credentials = execute_authorization_flow().await?;
        Ok(Client::new_from_credentials(credentials)?)
    }

    pub fn new_from_credentials(credentials: Credentials) -> anyhow::Result<Client> {
        log::debug!("Setting up Asana client...");
        Ok(Client {
            base_url: ASANA_API_BASE_URL.to_string(),
            inner_client: Client::construct_inner_client()?,
            credentials,
            last_refresh_attempt: None,
        })
    }

    /// Get a reference to the credentials that power this client.
    #[must_use]
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// Refresh this client's access token using the refresh token.
    ///
    /// If no refresh token is available, goes through the entire authorization flow again.
    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        log::debug!("Attempting to refresh the Asana access token...");
        self.credentials = if let Some(refresh_token) = &self.credentials.refresh_token {
            log::debug!("Found a refresh token, attempting to refresh authorization directly...");
            refresh_authorization(&oauth2::RefreshToken::new(refresh_token.clone())).await?
        } else {
            log::debug!("Could not find a refresh token, reinitiating the authorization flow...");
            execute_authorization_flow().await?
        };
        self.inner_client = Client::construct_inner_client()?;
        Ok(())
    }

    pub async fn get<'a, D: DataRequest<'a>>(
        &mut self,
        request_data: D::RequestData,
    ) -> anyhow::Result<D::ResponseData> {
        let endpoint = D::endpoint(request_data, &Url::parse(&self.base_url.clone())?);

        let mut query = D::other_params();
        query.insert("opt_fields".to_string(), D::fields().join(","));

        log::debug!("Making a request to {endpoint} with {query:?} params...");
        let response = self
            .inner_client
            .get(endpoint.clone())
            .bearer_auth(&self.credentials.access_token)
            .query(&query)
            .send()
            .await
            .context("failed to make request")?;

        let response = if response.status() == StatusCode::UNAUTHORIZED {
            if self
                .last_refresh_attempt
                .is_some_and(|t| t + Duration::minutes(5) > Local::now())
            {
                return Err(ClientError::UnableToRefreshToken)?;
            }
            self.refresh().await?;
            self.inner_client
                .get(endpoint)
                .bearer_auth(&self.credentials.access_token)
                .query(&query)
                .send()
                .await
                .context("failed to make request")?
        } else {
            response
        };

        Ok(response.json::<DataResponse<D::ResponseData>>().await?.data)
    }
}

pub mod datetime_format {
    use chrono::{DateTime, Local, NaiveDateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    const FORMAT: &str = "%Y-%m-%dT%H:%M:%S.%fZ";

    pub fn serialize<S>(date: &DateTime<Local>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("{}", date.naive_utc().format(FORMAT));
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Local>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let dt = NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)?;
        Ok(DateTime::from(DateTime::<Utc>::from_naive_utc_and_offset(
            dt, Utc,
        )))
    }
}

pub mod optional_date_format {
    use chrono::NaiveDate;
    use serde::{self, Deserialize, Deserializer, Serializer};

    const FORMAT: &str = "%Y-%m-%d";

    pub fn serialize<S>(date: &Option<NaiveDate>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(date) = date {
            let s = format!("{}", date.format(FORMAT));
            serializer.serialize_str(&s)
        } else {
            serializer.serialize_unit()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
    where
        D: Deserializer<'de>,
    {
        if let Ok(s) = String::deserialize(deserializer) {
            Ok(Some(
                NaiveDate::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)?,
            ))
        } else {
            Ok(None)
        }
    }
}
