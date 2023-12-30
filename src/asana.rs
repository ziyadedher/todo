//! Simple interactions with the Asana API.
//!
//! This module provides a client for interacting with the Asana API. It also provides a set of types that can be used
//! to make requests to the Asana API.
//!
//! # Examples
//!
//! The following example shows how to use the client to get all the names of incomplete tasks in a user's task list
//! along with some information about when they were created and when they are due.
//!
//! ```no_run
//! # use asana_api::asana::{Client, DataRequest};
//! # use serde::{Deserialize, Serialize};
//! # use todo::asana::execute_authorization_flow;
//!
//! #[derive(Debug, Deserialize, Serialize)]
//! struct Task {
//!     gid: String,
//!     #[serde(with = "todo::asana::serde_formats::datetime")]
//!     created_at: DateTime<Local>,
//!     #[serde(with = "todo::asana::serde_formats::optional_date")]
//!     due_on: Option<NaiveDate>,
//!     name: String,
//! }
//!
//! impl<'a> DataRequest<'a> for Task {
//!     type RequestData = String;
//!     type ResponseData = Vec<Task>;
//!
//!     fn segments(request_data: &'a Self::RequestData) -> Vec<String> {
//!         vec![
//!             "user_task_lists".to_string(),
//!             request_data.clone(),
//!             "tasks".to_string(),
//!         ]
//!     }
//!
//!     fn fields() -> &'a [&'a str] {
//!         &["this.gid", "this.created_at", "this.due_on", "this.name"]
//!     }
//!
//!     fn params() -> &'a [(&'a str, &'a str)] {
//!         &[("completed_since", "now")]
//!     }
//! }
//!
//! # async fn run() -> anyhow::Result<()> {
//! let credentials = execute_authorization_flow().await?;
//! let mut client = Client::new(credentials)?;
//! let tasks: Vec<Task> = client.get::<Task>("user_task_list_gid".to_string()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # See Also
//!
//! - [Asana API documentation](https://developers.asana.com/docs)

use std::io::{self, Write};

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

/// Comprehensive set of authorization credentials for the client.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Credentials {
    /// OAuth2 authorization credentials for the client.
    OAuth2 {
        /// OAuth2 access token, read more at https://oauth.net/2/access-tokens/
        access_token: String,
        /// OAuth2 refresh token, read more at https://oauth.net/2/refresh-tokens/
        refresh_token: Option<String>,
    },
    /// Personal access token, read more at https://developers.asana.com/docs/personal-access-token
    PersonalAccessToken(String),
}

/// Execute the full `OAuth2` authorization flow.
///
/// This function will open the user's browser to the Asana authorization page, and wait for the user to provide the
/// authorization code. Once the user has provided the authorization code, it will exchange it for access credentials
/// and return those credentials.
///
/// # Errors
///
/// This function will return an error if the authorization code could not be exchanged for access credentials.
///
/// # Examples
///
/// ```no_run
/// # use asana_api::asana::execute_authorization_flow;
/// # async fn run() -> anyhow::Result<()> {
/// let credentials = execute_authorization_flow().await?;
/// # Ok(())
/// # }
/// ```
///
/// # Panics
///
/// This function will panic if it is unable to open the user's browser.
///
/// # See Also
///
/// - [Asana OAuth2 documentation](https://developers.asana.com/docs/oauth)
/// - [OAuth2 documentation](https://oauth.net/2/)
/// - [OAuth2 RFC](https://tools.ietf.org/html/rfc6749)
/// - [OAuth2 for Native Apps RFC](https://tools.ietf.org/html/rfc8252)
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
    open::that_detached(auth_url.to_string())
        .context("could not open authorization URL in the browser")?;

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
    let credentials = Credentials::OAuth2 {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token
            .refresh_token()
            .map(|token| token.secret().to_string()),
    };

    Ok(credentials)
}

/// Refresh the access token using the refresh token.
///
/// # Errors
///
/// This function will return an error if the refresh token could not be exchanged for access credentials.
///
/// # Examples
///
/// ```no_run
/// # use asana_api::asana::refresh_authorization;
/// # async fn run() -> anyhow::Result<()> {
/// let credentials = refresh_authorization(&"refresh_token".to_string()).await?;
/// # Ok(())
/// # }
/// ```
///
/// # See Also
///
/// - [Asana OAuth2 documentation](https://developers.asana.com/docs/oauth)
/// - [OAuth2 documentation](https://oauth.net/2/)
/// - [OAuth2 RFC](https://tools.ietf.org/html/rfc6749)
/// - [OAuth2 for Native Apps RFC](https://tools.ietf.org/html/rfc8252)
/// - [OAuth2 Refresh Token RFC](https://tools.ietf.org/html/rfc6749#section-6)
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
    let credentials = Credentials::OAuth2 {
        access_token: token.access_token().secret().to_string(),
        refresh_token: Some(
            token
                .refresh_token()
                .unwrap_or(refresh_token)
                .secret()
                .to_string(),
        ),
    };

    Ok(credentials)
}

/// Trait for types that can be used to make requests to the Asana API.
///
/// # Examples
///
/// The following example shows how to implement `DataRequest` for a type that can be used to request all the names of
/// incomplete tasks in a user's task list.
///
/// ```no_run
/// # use asana_api::asana::DataRequest;
/// # use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize, Serialize)]
/// struct Task {
///     name: String,
/// }
///
/// impl<'a> DataRequest<'a> for Task {
///     type RequestData = String;
///     type ResponseData = Vec<Task>;
///
///     fn segments(request_data: &'a Self::RequestData) -> Vec<String> {
///         vec![
///             "user_task_lists".to_string(),
///             request_data.clone(),
///             "tasks".to_string(),
///         ]
///     }
///
///     fn fields() -> &'a [&'a str] {
///         &["this.name"]
///     }
///
///     fn params() -> &'a [(&'a str, &'a str)] {
///         &[("completed_since", "now")]
///     }
/// }
/// ```
///
/// # See Also
/// - [Asana API documentation](https://developers.asana.com/docs)
/// - [Asana API reference](https://developers.asana.com/reference)
/// - [Asana API explorer](https://developers.asana.com/explorer)
pub trait DataRequest<'a> {
    /// Type of additonal data that is required to make the request.
    type RequestData: 'a;
    /// Type of data that is returned by the request.
    type ResponseData: Serialize + DeserializeOwned;

    /// Get the segments of the URL that are required to make the request.
    #[must_use]
    fn segments(request_data: &'a Self::RequestData) -> Vec<String>;

    /// Get the fields to query the Asana API for.
    ///
    /// This should line up with the fields in `ResponseData` and must follow the `opt_fields` described in the [Asana
    /// API input/output options documentation](https://developers.asana.com/docs/inputoutput-options).
    #[must_use]
    fn fields() -> &'a [&'a str];

    /// Get any additional query parameters to use when making the request.
    #[must_use]
    fn params() -> &'a [(&'a str, &'a str)] {
        &[]
    }
}

#[derive(Deserialize, Serialize)]
struct DataResponse<D> {
    data: D,
}

#[derive(Debug, Error)]
enum ClientError {
    #[error("unable to refresh access token: {0}")]
    UnableToRefreshAccessToken(String),
}

/// Client for the Asana API.
///
/// This client is used to make requests to the Asana API and handles refreshing the access token when it expires. It
/// also handles the serialization and deserialization of data to and from the Asana API.
///
/// The primary entry point for this client is the [`get`](Client::get) method, which is used to make requests to the
/// Asana API. This method is based on a type that implements the [`DataRequest`](DataRequest) trait, which is used to
/// specify the data that is required to make the request, the data that is returned by the request, and the fields to
/// query the Asana API for.
///
/// # Examples
///
/// The following example shows how to use the client to get all the names of incomplete tasks in a user's task list.
///
/// ```no_run
/// # use asana_api::asana::{Client, DataRequest};
/// # use serde::{Deserialize, Serialize};
/// # use todo::asana::execute_authorization_flow;
///
/// #[derive(Deserialize, Serialize)]
/// struct Task {
///     name: String,
/// }
///
/// impl<'a> DataRequest<'a> for Task {
///     type RequestData = String;
///     type ResponseData = Vec<Task>;
///
///     fn segments(request_data: &'a Self::RequestData) -> Vec<String> {
///         vec![
///             "user_task_lists".to_string(),
///             request_data.clone(),
///             "tasks".to_string(),
///         ]
///     }
///
///     fn fields() -> &'static [&'static str] {
///         &["this.name"]
///     }
///
///     fn params() -> &'a [(&'a str, &'a str)] {
///         &[("completed_since", "now")]
///     }
/// }
///
/// # async fn run() -> anyhow::Result<()> {
/// let credentials = execute_authorization_flow().await?;
/// let mut client = Client::new(credentials)?;
/// let tasks: Vec<Task> = client.get::<Task>("user_task_list_gid".to_string()).await?;
/// # Ok(())
/// # }
/// ````
pub struct Client {
    base_url: Url,
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

    async fn make_request(&self, url: &Url) -> anyhow::Result<reqwest::Response> {
        let token = match &self.credentials {
            Credentials::OAuth2 {
                access_token,
                refresh_token: _,
            } => access_token,
            Credentials::PersonalAccessToken(token) => token,
        };
        self.inner_client
            .get(url.clone())
            .bearer_auth(token)
            .send()
            .await
            .context("failed to make request")
    }

    /// Create a new client with the given credentials.
    ///
    /// # Errors
    ///
    /// This function will return an error if the inner client could not be constructed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use asana_api::asana::Client;
    /// # use todo::asana::execute_authorization_flow;
    /// # async fn run() -> anyhow::Result<()> {
    /// let credentials = execute_authorization_flow().await?;
    /// let client = Client::new(credentials)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(credentials: Credentials) -> anyhow::Result<Client> {
        log::debug!("Setting up Asana client...");
        Ok(Client {
            base_url: Url::parse(ASANA_API_BASE_URL)?,
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

    /// Refresh the access token.
    ///
    /// If no refresh token is available, this will reinitiate the authorization flow.
    ///
    /// # Errors
    ///
    /// This function will return an error if the refresh token could not be exchanged for access credentials.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use asana_api::asana::Client;
    /// # use todo::asana::execute_authorization_flow;
    /// # async fn run() -> anyhow::Result<()> {
    /// let credentials = execute_authorization_flow().await?;
    /// let mut client = Client::new(credentials)?;
    /// client.refresh().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        match &self.credentials {
            Credentials::OAuth2 {
                access_token: _,
                refresh_token,
            } => {
                log::debug!("Attempting to refresh the Asana access token...");
                self.credentials = if let Some(refresh_token) = refresh_token {
                    log::debug!(
                        "Found a refresh token, attempting to refresh authorization directly..."
                    );
                    refresh_authorization(&oauth2::RefreshToken::new(refresh_token.clone())).await?
                } else {
                    log::debug!(
                        "Could not find a refresh token, reinitiating the authorization flow..."
                    );
                    execute_authorization_flow().await?
                };
                self.inner_client = Client::construct_inner_client()?;
                Ok(())
            }

            _ => {
                return Err(ClientError::UnableToRefreshAccessToken(
                    "not using OAuth2 flow".to_string(),
                ))?
            }
        }
    }

    /// Make a request to the Asana API.
    ///
    /// See documentation for [`DataRequest`](DataRequest) and [`Client`](Client) for more information on how to use
    /// this method.
    ///
    /// # Errors
    ///
    /// This function will return an error if the request could not be made or if the response could not be
    /// deserialized.
    pub async fn get<'a, D: DataRequest<'a> + 'a>(
        &mut self,
        request_data: &'a D::RequestData,
    ) -> anyhow::Result<D::ResponseData> {
        let mut url = self.base_url.join(&D::segments(request_data).join("/"))?;

        let fields = D::fields().join(",");
        let query = &[D::params(), &[("opt_fields", &fields)]].concat();
        url.query_pairs_mut().extend_pairs(query).finish();

        log::debug!("Making a request to {url}...");
        let response = self.make_request(&url).await?;

        let response = if response.status() == StatusCode::UNAUTHORIZED {
            if self
                .last_refresh_attempt
                .is_some_and(|t| t + Duration::minutes(5) > Local::now())
            {
                return Err(ClientError::UnableToRefreshAccessToken(
                    "unauthorized".to_string(),
                ))?;
            }
            self.refresh().await?;
            self.make_request(&url).await?
        } else {
            response
        };

        Ok(response.json::<DataResponse<D::ResponseData>>().await?.data)
    }
}

/// Definitions for for the serde serialization and deserialization of types that interact with the Asana API.
pub mod serde_formats {
    #![allow(missing_docs)]
    #![allow(clippy::missing_errors_doc)]
    pub mod datetime {
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

    pub mod optional_date {
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
}
