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
//! # use serde::{Deserialize, Serialize};
//! # use chrono::{DateTime, Local, NaiveDate};
//! # use todo::asana::{Client, DataRequest};
//! # use todo::asana::execute_authorization_flow;
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
//!     fn params(_request_data: &'a Self::RequestData) -> Vec<(&'a str, String)> {
//!         vec![("completed_since", "now".to_string())]
//!     }
//! }
//!
//! # async fn run() -> anyhow::Result<()> {
//! let credentials = execute_authorization_flow().await?;
//! let mut client = Client::new(credentials)?;
//! let tasks: Vec<Task> = client.get::<Task>(&"user_task_list_gid".to_string()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # See Also
//!
//! - [Asana API documentation](https://developers.asana.com/docs)

use anyhow::Context;
use chrono::{DateTime, Duration, Local};
use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, Input};
use oauth2::TokenResponse;
use reqwest::{Method, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

const API_BASE_URL: &str = "https://app.asana.com/api/1.0/";
const OAUTH_AUTHORIZATION_URL: &str = "https://app.asana.com/-/oauth_authorize";
const OAUTH_TOKEN_URL: &str = "https://app.asana.com/-/oauth_token";
const OAUTH_LOCAL_REDIRECT_URI: &str = "urn:ietf:wg:oauth:2.0:oob";

const APP_CLIENT_ID: &str = "1206215514588292";
const APP_CLIENT_SECRET: &str = "8c7ea1c603de8462a3ba24f827ff1658";

/// Type alias for a fully configured `OAuth2` client with auth and token endpoints set.
type ConfiguredOAuthClient = oauth2::Client<
    oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
    oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
    oauth2::StandardTokenIntrospectionResponse<
        oauth2::EmptyExtraTokenFields,
        oauth2::basic::BasicTokenType,
    >,
    oauth2::StandardRevocableToken,
    oauth2::StandardErrorResponse<oauth2::RevocationErrorResponseType>,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

fn setup_oauth_client() -> anyhow::Result<ConfiguredOAuthClient> {
    log::debug!("Setting up OAuth client...");
    Ok(
        oauth2::basic::BasicClient::new(oauth2::ClientId::new(APP_CLIENT_ID.to_string()))
            .set_client_secret(oauth2::ClientSecret::new(APP_CLIENT_SECRET.to_string()))
            .set_auth_uri(oauth2::AuthUrl::new(OAUTH_AUTHORIZATION_URL.to_string())?)
            .set_token_uri(oauth2::TokenUrl::new(OAUTH_TOKEN_URL.to_string())?)
            .set_redirect_uri(oauth2::RedirectUrl::new(
                OAUTH_LOCAL_REDIRECT_URI.to_string(),
            )?),
    )
}

/// Comprehensive set of authorization credentials for the client.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Credentials {
    /// `OAuth2` authorization credentials for the client.
    OAuth2 {
        /// `OAuth2` access token, read more at <https://oauth.net/2/access-tokens/>
        access_token: String,
        /// `OAuth2` refresh token, read more at <https://oauth.net/2/refresh-tokens/>
        refresh_token: Option<String>,
    },
    /// Personal access token, read more at <https://developers.asana.com/docs/personal-access-token>
    PersonalAccessToken(String),
}

/// Ask the user for a personal access token.
///
/// This opens the user's browser to the Asana personal access token page and prompts the user to enter the personal
/// access token they got from the page. Note that the personal access token flow is discouraged, and the OAuth flow
/// should be used instead.
///
/// # Errors
///
/// This function will return an error if the user could not be prompted for the personal access token.
///
/// # Examples
///
/// ```no_run
/// # use todo::asana::ask_for_pat;
/// # async fn run() -> anyhow::Result<()> {
/// let pat = ask_for_pat()?;
/// # Ok(())
/// # }
/// ```
/// # See Also
///
/// - [Asana API PAT documentation](https://developers.asana.com/docs/personal-access-token)
pub fn ask_for_pat() -> anyhow::Result<Credentials> {
    let pat_page_url = Url::parse("https://app.asana.com/0/my-apps")?;

    log::info!("Opening browser to PAT page...");
    Term::stdout().write_line(
      &style(format!(
          "Opening your browser and sending you to {pat_page_url}...\nGenerate a new personal access token, and once you're done, come back here and post the token you got."
      ))
      .dim()
      .to_string(),
  )?;
    open::that_detached(pat_page_url.to_string())
        .context("could not PAT page URL in the browser")?;

    let pat = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("personal access token")
        .interact_text()?;
    Ok(Credentials::PersonalAccessToken(pat))
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
/// # use todo::asana::execute_authorization_flow;
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
    let oauth_client = setup_oauth_client()?;

    log::debug!("Setting up authorization request...");
    let (pkce_challenge, pkce_verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
    let (auth_url, _) = oauth_client
        .authorize_url(oauth2::CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge)
        .url();

    log::info!("Opening browser to authorization URL...");
    Term::stdout().write_line(
        &style(format!(
            "Opening your browser and sending you to {auth_url}...\nOnce you're done, come back here and post the code you got."
        ))
        .dim()
        .to_string(),
    )?;
    open::that_detached(auth_url.to_string())
        .context("could not open authorization URL in the browser")?;

    log::info!("Waiting for user to provide the authorization code...");
    let auth_code = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("auth code")
        .interact_text()?;

    log::info!("Exchanging authorization code for an access token...");
    let http_client = oauth2::reqwest::Client::new();
    let token = oauth_client
        .exchange_code(oauth2::AuthorizationCode::new(auth_code.trim().to_string()))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&http_client)
        .await
        .context("could not exchange authorization code for an access token")?;
    let credentials = Credentials::OAuth2 {
        access_token: token.access_token().secret().clone(),
        refresh_token: token.refresh_token().map(|token| token.secret().clone()),
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
/// # use todo::asana::refresh_authorization;
/// # async fn run() -> anyhow::Result<()> {
/// let refresh_token = oauth2::RefreshToken::new("refresh_token".to_string());
/// let credentials = refresh_authorization(&refresh_token).await?;
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
    let oauth_client = setup_oauth_client()?;
    let http_client = oauth2::reqwest::Client::new();
    let token = oauth_client
        .exchange_refresh_token(refresh_token)
        .request_async(&http_client)
        .await
        .context("could not exchange refresh token for an access token")?;
    let credentials = Credentials::OAuth2 {
        access_token: token.access_token().secret().clone(),
        refresh_token: Some(
            token
                .refresh_token()
                .unwrap_or(refresh_token)
                .secret()
                .clone(),
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
/// # use todo::asana::DataRequest;
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
///     fn params(_request_data: &'a Self::RequestData) -> Vec<(&'a str, String)> {
///         vec![("completed_since", "now".to_string())]
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
    fn params(_request_data: &'a Self::RequestData) -> Vec<(&'a str, String)> {
        vec![]
    }
}

/// Wrapper for data exchanged with the Asana API.
///
/// This wrapper is used to serialize data to the Asana API or deserialize from it, since the Asana API expects a
/// "data" field in a lot of cases.
#[derive(Deserialize, Serialize)]
pub struct DataWrapper<D> {
    /// Data exchanged with the Asana API
    pub data: D,
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
/// Asana API. This method is based on a type that implements the [`DataRequest`] trait, which is used to
/// specify the data that is required to make the request, the data that is returned by the request, and the fields to
/// query the Asana API for.
///
/// # Examples
///
/// The following example shows how to use the client to get all the names of incomplete tasks in a user's task list.
///
/// ```no_run
/// # use todo::asana::{Client, DataRequest};
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
///     fn params(_request_data: &'a Self::RequestData) -> Vec<(&'a str, String)> {
///         vec![("completed_since", "now".to_string())]
///     }
/// }
///
/// # async fn run() -> anyhow::Result<()> {
/// let credentials = execute_authorization_flow().await?;
/// let mut client = Client::new(credentials)?;
/// let tasks: Vec<Task> = client.get::<Task>(&"user_task_list_gid".to_string()).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    base_url: Url,
    credentials: Credentials,
    http: reqwest::Client,

    last_refresh_attempt: Option<DateTime<Local>>,
}

impl Client {
    fn construct_http() -> anyhow::Result<reqwest::Client> {
        reqwest::ClientBuilder::new()
            .connect_timeout(Duration::seconds(5).to_std()?)
            .timeout(Duration::seconds(10).to_std()?)
            .build()
            .context("could not build Asana client")
    }

    fn get_authorization_token(&self) -> &str {
        match &self.credentials {
            Credentials::OAuth2 {
                access_token,
                refresh_token: _,
            } => access_token,
            Credentials::PersonalAccessToken(token) => token,
        }
    }

    async fn make_get_request(&self, url: &Url) -> anyhow::Result<reqwest::Response> {
        self.http
            .get(url.clone())
            .bearer_auth(self.get_authorization_token())
            .send()
            .await
            .context("failed to make request")
    }

    /// Make a POST or PUT request to the Asana API.
    ///
    /// # Errors
    ///
    /// This function will return an error if the request could not be made.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use todo::asana::Client;
    /// # use todo::asana::execute_authorization_flow;
    /// # use serde::Serialize;
    /// # async fn run() -> anyhow::Result<()> {
    /// let credentials = execute_authorization_flow().await?;
    /// let mut client = Client::new(credentials)?;
    ///
    /// #[derive(Serialize)]
    /// struct TaskCreation {
    ///     name: String,
    /// }
    ///
    /// let response = client.mutate_request(reqwest::Method::POST, &"https://app.asana.com/api/1.0/tasks".parse()?, TaskCreation {
    ///     name: "test".to_string(),
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn mutate_request(
        &self,
        method: Method,
        url: &Url,
        body: impl Serialize,
    ) -> anyhow::Result<reqwest::Response> {
        self.http
            .request(method, url.clone())
            .bearer_auth(self.get_authorization_token())
            .json(&body)
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
    /// # use todo::asana::Client;
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
            base_url: Url::parse(API_BASE_URL)?,
            http: Client::construct_http()?,
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
    /// # use todo::asana::Client;
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
                self.http = Client::construct_http()?;
                Ok(())
            }

            Credentials::PersonalAccessToken(_pat) => Err(ClientError::UnableToRefreshAccessToken(
                "not using OAuth2 flow".to_string(),
            )
            .into()),
        }
    }

    /// Make a request to the Asana API.
    ///
    /// See documentation for [`DataRequest`] and [`Client`] for more information on how to use
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
        let query = &[D::params(request_data), vec![("opt_fields", fields)]].concat();
        url.query_pairs_mut().extend_pairs(query).finish();

        log::debug!("Making a request to {url}...");
        let response = self.make_get_request(&url).await?;

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
            self.make_get_request(&url).await?
        } else {
            response
        };

        Ok(response.json::<DataWrapper<D::ResponseData>>().await?.data)
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestDateTime {
        #[serde(with = "serde_formats::datetime")]
        timestamp: DateTime<Local>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestOptionalDate {
        #[serde(with = "serde_formats::optional_date")]
        date: Option<NaiveDate>,
    }

    #[test]
    fn datetime_deserializes_asana_format() {
        let json = r#"{"timestamp": "2024-06-15T14:30:00.000Z"}"#;
        let parsed: TestDateTime = serde_json::from_str(json).unwrap();

        // The parsed time should be 2024-06-15 14:30:00 UTC converted to local
        let expected_utc = chrono::Utc
            .with_ymd_and_hms(2024, 6, 15, 14, 30, 0)
            .unwrap();
        assert_eq!(parsed.timestamp.with_timezone(&chrono::Utc), expected_utc);
    }

    #[test]
    fn datetime_serializes_to_asana_format() {
        let utc_time = chrono::Utc
            .with_ymd_and_hms(2024, 6, 15, 14, 30, 0)
            .unwrap();
        let local_time: DateTime<Local> = utc_time.into();
        let test = TestDateTime {
            timestamp: local_time,
        };

        let json = serde_json::to_string(&test).unwrap();
        assert!(json.contains("2024-06-15T14:30:00.000000000Z"));
    }

    #[test]
    fn optional_date_deserializes_present_date() {
        let json = r#"{"date": "2024-06-15"}"#;
        let parsed: TestOptionalDate = serde_json::from_str(json).unwrap();

        assert_eq!(
            parsed.date,
            Some(NaiveDate::from_ymd_opt(2024, 6, 15).unwrap())
        );
    }

    #[test]
    fn optional_date_deserializes_null() {
        let json = r#"{"date": null}"#;
        let parsed: TestOptionalDate = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.date, None);
    }

    #[test]
    fn optional_date_serializes_present_date() {
        let test = TestOptionalDate {
            date: Some(NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()),
        };

        let json = serde_json::to_string(&test).unwrap();
        assert_eq!(json, r#"{"date":"2024-06-15"}"#);
    }

    #[test]
    fn optional_date_serializes_none() {
        let test = TestOptionalDate { date: None };

        let json = serde_json::to_string(&test).unwrap();
        assert_eq!(json, r#"{"date":null}"#);
    }
}
