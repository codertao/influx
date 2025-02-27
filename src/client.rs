use crate::{query::Query, Measurement};
use isahc::{AsyncReadResponseExt, HttpClient};
use std::{collections::HashMap, error::Error, fmt::Display};

pub type InfluxQueryResponse = Vec<HashMap<String, String>>;

#[derive(Clone)]
pub struct InfluxClient {
    url: String,
    key: String,
    org: String,
    http_client: HttpClient,
}

impl InfluxClient {
    fn new(url: String, key: String, org: String, http_client: HttpClient) -> Self {
        Self {
            url,
            key,
            org,
            http_client,
        }
    }

    pub fn builder(url: String, key: String, org: String) -> InfluxClientBuilder {
        InfluxClientBuilder::new(url, key, org)
    }

    /// Write data to the specified bucket.
    pub async fn write(
        &self,
        bucket: &str,
        measurements: &[Measurement],
    ) -> Result<(), InfluxError> {
        let payload = measurements
            .iter()
            .map(|m| m.to_line_protocol())
            .collect::<Vec<_>>()
            .join("\n");
        let url = format!(
            "{}/api/v2/write?org={}&bucket={}&precision=ms",
            self.url, self.org, bucket
        );

        trace!("posting payload to influx at '{}': '{}'", url, payload);

        let request = isahc::Request::builder()
            .uri(url)
            .method("POST")
            .header("Authorization", format!("Token {}", &self.key))
            .body(payload)?;
        let mut response = self.http_client.send_async(request).await?;
        if !response.status().is_success() {
            let body = response.text().await?;
            return Err(InfluxError::NonSuccessResponse(response.status(), body));
        }
        Ok(())
    }

    pub async fn query(&self, query: Query) -> Result<InfluxQueryResponse, InfluxError> {
        let payload = query.to_string();
        self.raw_query(&payload).await
    }

    pub async fn raw_query(&self, query: &str) -> Result<InfluxQueryResponse, InfluxError> {
        let url = format!("{}/api/v2/query?org={}", self.url, self.org);
        trace!("posting query to influx at '{}': '{}'", url, query);

        let request = isahc::Request::builder()
            .uri(&url)
            .method("POST")
            .header("Authorization", format!("Token {}", &self.key))
            .header("Content-Type", "application/vnd.flux")
            .header("Accept", "application/csv")
            .body(query)?;

        let mut response = self.http_client.send_async(request).await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            return Err(InfluxError::NonSuccessResponse(status, body));
        }

        let body = response.text().await?;

        trace!("response body: '{}'", body);

        let lines: Vec<String> = body.lines().map(|l| l.trim().to_owned()).collect();
        let tables: Vec<_> = lines
            .split(|t| t.is_empty())
            .filter(|t| !t.is_empty())
            .map(|t| t.join("\n"))
            .collect();

        let mut records = Vec::new();
        for table in tables {
            let mut reader = csv::Reader::from_reader(table.as_bytes());
            for result in reader.deserialize() {
                let mut record: HashMap<String, String> = result?;
                record.remove("");
                records.push(record);
            }
        }

        Ok(records)
    }
}

#[derive(Clone)]
pub struct InfluxClientBuilder {
    url: String,
    key: String,
    org: String,
}

impl InfluxClientBuilder {
    fn new(url: String, key: String, org: String) -> Self {
        Self { url, key, org }
    }

    pub fn build(self) -> Result<InfluxClient, InfluxClientBuilderError> {
        Ok(InfluxClient::new(
            self.url,
            self.key,
            self.org,
            isahc::HttpClient::new().unwrap(),
        ))
    }
}

#[derive(Debug)]
pub enum InfluxError {
    HttpError(isahc::http::Error),
    IsahcError(isahc::Error),
    IoError(std::io::Error),
    CsvError(csv::Error),
    NonSuccessResponse(isahc::http::StatusCode, String),
}

impl Error for InfluxError {}

impl From<isahc::Error> for InfluxError {
    fn from(err: isahc::Error) -> Self {
        Self::IsahcError(err)
    }
}

impl From<std::io::Error> for InfluxError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<isahc::http::Error> for InfluxError {
    fn from(err: isahc::http::Error) -> Self {
        Self::HttpError(err)
    }
}

impl From<csv::Error> for InfluxError {
    fn from(err: csv::Error) -> Self {
        Self::CsvError(err)
    }
}

impl Display for InfluxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let output = match self {
            InfluxError::NonSuccessResponse(status, body) => {
                format!("non-success response: '{}', body: '{}'", status, body)
            }
            InfluxError::CsvError(err) => format!("csv error: '{}'", err),
            InfluxError::HttpError(err) => format!("http error: '{}'", err),
            InfluxError::IsahcError(err) => format!("isahc error: '{}'", err),
            InfluxError::IoError(err) => format!("io error: '{}'", err),
        };

        write!(f, "{}", output)
    }
}

#[derive(Debug)]
pub enum InfluxClientBuilderError {}

impl Error for InfluxClientBuilderError {}

impl Display for InfluxClientBuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error building influx client")
    }
}
