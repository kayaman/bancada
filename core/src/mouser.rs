//! Typed client for Mouser's Search API.
//!
//! The public model deliberately contains live lookup facts only: manufacturer
//! identity, stock, lifecycle, links, attributes, and price breaks. None of
//! these fields is electrical proof, and callers must not persist them as
//! Mouser's API terms prohibit caching or storing returned catalog content.
//! Circuit validation continues to require user-entered, datasheet-verified
//! declarations in `hardware/circuit.yaml`.
//!
//! Contract: <https://api.mouser.com/api/docs/V1>. Mouser requires the Search
//! API key as a query parameter; all error paths therefore avoid rendering a
//! request URL.

use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const DEFAULT_SEARCH_ROOT: &str = "https://api.mouser.com/api/v1/search";
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouserSearchKind {
    Keyword,
    PartNumber,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouserSearchOption {
    #[default]
    None,
    Rohs,
    InStock,
    RohsAndInStock,
}

impl MouserSearchOption {
    fn api_value(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Rohs => "Rohs",
            Self::InStock => "InStock",
            Self::RohsAndInStock => "RohsAndInStock",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MouserSearchRequest {
    pub query: String,
    pub kind: MouserSearchKind,
    #[serde(default = "default_records")]
    pub records: u8,
    #[serde(default)]
    pub starting_record: u32,
    #[serde(default)]
    pub option: MouserSearchOption,
    /// Part-number search only. Requests Mouser's `Exact` matching mode.
    #[serde(default)]
    pub exact: bool,
}

const fn default_records() -> u8 {
    10
}

impl MouserSearchRequest {
    pub fn validate(&self) -> Result<()> {
        let query = self.query.trim();
        let query_len = query.chars().count();
        if query.is_empty() {
            return Err(Error::Other("enter a Mouser search term".into()));
        }
        if query.chars().any(char::is_control) {
            return Err(Error::Other(
                "Mouser search terms cannot contain control characters".into(),
            ));
        }
        if !(1..=50).contains(&self.records) {
            return Err(Error::Other(
                "Mouser searches must request between 1 and 50 records".into(),
            ));
        }
        match self.kind {
            MouserSearchKind::Keyword if query_len > 200 => Err(Error::Other(
                "Bancada limits Mouser keyword searches to 200 characters".into(),
            )),
            MouserSearchKind::PartNumber if !(3..=40).contains(&query_len) => Err(Error::Other(
                "Mouser part numbers must be between 3 and 40 characters".into(),
            )),
            MouserSearchKind::PartNumber if query.contains('|') => Err(Error::Other(
                "Bancada accepts one Mouser part number per lookup".into(),
            )),
            MouserSearchKind::PartNumber if self.option != MouserSearchOption::None => Err(
                Error::Other("RoHS and stock filters apply only to keyword searches".into()),
            ),
            _ => Ok(()),
        }
    }

    fn endpoint(&self) -> &'static str {
        match self.kind {
            MouserSearchKind::Keyword => "keyword",
            MouserSearchKind::PartNumber => "partnumber",
        }
    }

    fn body(&self) -> String {
        let query = self.query.trim();
        match self.kind {
            MouserSearchKind::Keyword => serde_json::json!({
                "SearchByKeywordRequest": {
                    "keyword": query,
                    "records": self.records,
                    "startingRecord": self.starting_record,
                    "searchOptions": self.option.api_value(),
                    "searchWithYourSignUpLanguage": "false",
                    "mouserPaysCustomsAndDuties": false
                }
            }),
            MouserSearchKind::PartNumber => serde_json::json!({
                "SearchByPartRequest": {
                    "mouserPartNumber": query,
                    "partSearchOptions": if self.exact { "Exact" } else { "None" },
                    "mouserPaysCustomsAndDuties": false
                }
            }),
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MouserPriceBreak {
    pub quantity: u32,
    pub price: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MouserAttribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MouserPart {
    pub mouser_part_number: String,
    pub manufacturer_part_number: String,
    pub manufacturer: String,
    pub description: String,
    pub category: String,
    pub availability: String,
    pub in_stock: String,
    pub factory_stock: String,
    pub lifecycle_status: String,
    pub lead_time: String,
    pub minimum_order_quantity: String,
    pub order_multiple: String,
    pub datasheet_url: String,
    pub product_url: String,
    pub image_url: String,
    pub rohs_status: String,
    pub suggested_replacement: String,
    pub discontinued: String,
    pub reeling: Option<bool>,
    pub price_breaks: Vec<MouserPriceBreak>,
    pub attributes: Vec<MouserAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MouserSearchResponse {
    pub total: u32,
    pub parts: Vec<MouserPart>,
}

/// Validate without returning or logging the key. Search keys are currently
/// UUID-like, but Mouser documents this parameter as an opaque string, so the
/// client intentionally does not impose a UUID format.
pub fn validate_api_key(key: &str) -> Result<&str> {
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::Other(
            "configure a Mouser Search API key before searching".into(),
        ));
    }
    if key.len() > 256 || key.chars().any(char::is_control) {
        return Err(Error::Other("the Mouser API key is not valid".into()));
    }
    Ok(key)
}

pub struct MouserClient {
    api_key: String,
    search_root: String,
    agent: ureq::Agent,
}

impl std::fmt::Debug for MouserClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouserClient")
            .field("api_key", &"[redacted]")
            .field("search_root", &self.search_root)
            .finish_non_exhaustive()
    }
}

impl MouserClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::with_search_root(api_key, DEFAULT_SEARCH_ROOT)
    }

    fn with_search_root(api_key: impl Into<String>, search_root: &str) -> Result<Self> {
        let api_key = api_key.into();
        let api_key = validate_api_key(&api_key)?.to_string();
        let search_root = search_root.trim_end_matches('/');
        if !(search_root.starts_with("https://")
            || cfg!(test) && search_root.starts_with("http://"))
        {
            return Err(Error::Other(
                "the Mouser API endpoint must use HTTPS".into(),
            ));
        }
        Ok(Self {
            api_key,
            search_root: search_root.to_string(),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(20))
                .timeout_write(Duration::from_secs(10))
                .build(),
        })
    }

    pub fn search(&self, request: &MouserSearchRequest) -> Result<MouserSearchResponse> {
        request.validate()?;
        let url = format!("{}/{}", self.search_root, request.endpoint());
        let response = self
            .agent
            .post(&url)
            .query("apiKey", &self.api_key)
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_string(&request.body());

        let response = match response {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) => {
                let body = read_response(response)?;
                let detail = api_error_message(&body, Some(&self.api_key))
                    .unwrap_or_else(|| format!("Mouser returned HTTP {status}"));
                return Err(Error::Other(detail));
            }
            Err(ureq::Error::Transport(error)) => {
                // `Transport`'s display form may include the request URL, whose
                // query contains the API key. Report only its non-secret kind.
                return Err(Error::Other(format!(
                    "Mouser API transport failed: {:?}",
                    error.kind()
                )));
            }
        };
        parse_response(&read_response(response)?, Some(&self.api_key))
    }
}

fn read_response(response: ureq::Response) -> Result<String> {
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_string(&mut body)?;
    if body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(Error::Other(
            "Mouser API response was unexpectedly large".into(),
        ));
    }
    Ok(body)
}

fn api_error_message(body: &str, secret: Option<&str>) -> Option<String> {
    let raw: RawResponse = serde_json::from_str(body).ok()?;
    let messages: Vec<String> = raw
        .errors
        .unwrap_or_default()
        .into_iter()
        .filter_map(|error| {
            let message = error.message.trim();
            (!message.is_empty()).then(|| {
                if error.code.trim().is_empty() {
                    message.to_string()
                } else {
                    format!("{}: {message}", error.code.trim())
                }
            })
        })
        .collect();
    (!messages.is_empty()).then(|| redact_secret(&messages.join("; "), secret))
}

fn redact_secret(value: &str, secret: Option<&str>) -> String {
    match secret.filter(|secret| !secret.is_empty()) {
        Some(secret) => value.replace(secret, "[redacted]"),
        None => value.to_string(),
    }
}

fn parse_response(body: &str, secret: Option<&str>) -> Result<MouserSearchResponse> {
    let raw: RawResponse = serde_json::from_str(body).map_err(|source| Error::Json {
        what: "Mouser Search API response".into(),
        source,
    })?;
    if let Some(message) = api_error_message(body, secret) {
        return Err(Error::Other(message));
    }
    let results = raw.search_results.unwrap_or_default();
    let parts = results
        .parts
        .unwrap_or_default()
        .into_iter()
        .map(MouserPart::try_from)
        .collect::<Result<Vec<_>>>()?;
    Ok(MouserSearchResponse {
        total: results.number_of_result,
        parts,
    })
}

#[derive(Default, Deserialize)]
struct RawResponse {
    #[serde(default, rename = "Errors")]
    errors: Option<Vec<RawError>>,
    #[serde(default, rename = "SearchResults")]
    search_results: Option<RawSearchResults>,
}

#[derive(Default, Deserialize)]
struct RawError {
    #[serde(default, rename = "Code")]
    code: String,
    #[serde(default, rename = "Message")]
    message: String,
}

#[derive(Default, Deserialize)]
struct RawSearchResults {
    #[serde(default, rename = "NumberOfResult")]
    number_of_result: u32,
    #[serde(default, rename = "Parts")]
    parts: Option<Vec<RawPart>>,
}

#[derive(Default, Deserialize)]
struct RawPart {
    #[serde(default, rename = "MouserPartNumber")]
    mouser_part_number: String,
    #[serde(default, rename = "ManufacturerPartNumber")]
    manufacturer_part_number: String,
    #[serde(default, rename = "Manufacturer")]
    manufacturer: String,
    #[serde(default, rename = "Description")]
    description: String,
    #[serde(default, rename = "Category")]
    category: String,
    #[serde(default, rename = "Availability")]
    availability: String,
    #[serde(default, rename = "AvailabilityInStock")]
    in_stock: String,
    #[serde(default, rename = "FactoryStock")]
    factory_stock: String,
    #[serde(default, rename = "LifecycleStatus")]
    lifecycle_status: String,
    #[serde(default, rename = "LeadTime")]
    lead_time: String,
    #[serde(default, rename = "Min")]
    minimum_order_quantity: String,
    #[serde(default, rename = "Mult")]
    order_multiple: String,
    #[serde(default, rename = "DataSheetUrl")]
    datasheet_url: String,
    #[serde(default, rename = "ProductDetailUrl")]
    product_url: String,
    #[serde(default, rename = "ImagePath")]
    image_url: String,
    #[serde(default, rename = "ROHSStatus")]
    rohs_status: String,
    #[serde(default, rename = "SuggestedReplacement")]
    suggested_replacement: String,
    #[serde(default, rename = "IsDiscontinued")]
    discontinued: String,
    #[serde(default, rename = "Reeling")]
    reeling: Option<bool>,
    #[serde(default, rename = "PriceBreaks")]
    price_breaks: Option<Vec<RawPriceBreak>>,
    #[serde(default, rename = "ProductAttributes")]
    attributes: Option<Vec<RawAttribute>>,
}

#[derive(Default, Deserialize)]
struct RawPriceBreak {
    #[serde(default, rename = "Quantity")]
    quantity: u32,
    #[serde(default, rename = "Price")]
    price: String,
    #[serde(default, rename = "Currency")]
    currency: String,
}

#[derive(Default, Deserialize)]
struct RawAttribute {
    #[serde(default, rename = "AttributeName")]
    name: String,
    #[serde(default, rename = "AttributeValue")]
    value: String,
}

fn validate_response_url(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let url = url::Url::parse(value)
        .map_err(|_| Error::Other(format!("Mouser returned an invalid {label} URL")))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(Error::Other(format!(
            "Mouser returned a non-HTTPS {label} URL"
        )));
    }
    Ok(())
}

impl TryFrom<RawPart> for MouserPart {
    type Error = Error;

    fn try_from(part: RawPart) -> Result<Self> {
        validate_response_url("datasheet", &part.datasheet_url)?;
        validate_response_url("product", &part.product_url)?;
        validate_response_url("image", &part.image_url)?;
        Ok(Self {
            mouser_part_number: part.mouser_part_number,
            manufacturer_part_number: part.manufacturer_part_number,
            manufacturer: part.manufacturer,
            description: part.description,
            category: part.category,
            availability: part.availability,
            in_stock: part.in_stock,
            factory_stock: part.factory_stock,
            lifecycle_status: part.lifecycle_status,
            lead_time: part.lead_time,
            minimum_order_quantity: part.minimum_order_quantity,
            order_multiple: part.order_multiple,
            datasheet_url: part.datasheet_url,
            product_url: part.product_url,
            image_url: part.image_url,
            rohs_status: part.rohs_status,
            suggested_replacement: part.suggested_replacement,
            discontinued: part.discontinued,
            reeling: part.reeling,
            price_breaks: part
                .price_breaks
                .unwrap_or_default()
                .into_iter()
                .map(|price| MouserPriceBreak {
                    quantity: price.quantity,
                    price: price.price,
                    currency: price.currency,
                })
                .collect(),
            attributes: part
                .attributes
                .unwrap_or_default()
                .into_iter()
                .map(|attribute| MouserAttribute {
                    name: attribute.name,
                    value: attribute.value,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: MouserSearchKind) -> MouserSearchRequest {
        MouserSearchRequest {
            query: "ESP32-C6-WROOM-1-N8".into(),
            kind,
            records: 10,
            starting_record: 0,
            option: if kind == MouserSearchKind::Keyword {
                MouserSearchOption::InStock
            } else {
                MouserSearchOption::None
            },
            exact: true,
        }
    }

    #[test]
    fn keyword_request_matches_the_official_root_and_option_names() {
        let body: serde_json::Value =
            serde_json::from_str(&request(MouserSearchKind::Keyword).body()).unwrap();
        assert_eq!(
            body["SearchByKeywordRequest"]["keyword"],
            "ESP32-C6-WROOM-1-N8"
        );
        assert_eq!(body["SearchByKeywordRequest"]["records"], 10);
        assert_eq!(body["SearchByKeywordRequest"]["searchOptions"], "InStock");
    }

    #[test]
    fn part_request_uses_exact_only_in_part_number_mode() {
        let body: serde_json::Value =
            serde_json::from_str(&request(MouserSearchKind::PartNumber).body()).unwrap();
        assert_eq!(
            body["SearchByPartRequest"]["mouserPartNumber"],
            "ESP32-C6-WROOM-1-N8"
        );
        assert_eq!(body["SearchByPartRequest"]["partSearchOptions"], "Exact");
    }

    #[test]
    fn input_limits_match_mouser_search_constraints() {
        let mut req = request(MouserSearchKind::PartNumber);
        req.query = "x".into();
        assert!(req.validate().unwrap_err().to_string().contains("3 and 40"));
        req.query = "x".repeat(41);
        assert!(req.validate().is_err());
        req.query = "ABC|DEF".into();
        assert!(req.validate().unwrap_err().to_string().contains("one"));
        req.query = "ABC".into();
        req.records = 51;
        assert!(req.validate().unwrap_err().to_string().contains("50"));
        req.records = 10;
        req.option = MouserSearchOption::InStock;
        assert!(req.validate().unwrap_err().to_string().contains("keyword"));
    }

    #[test]
    fn parses_and_normalizes_a_search_response() {
        let response = parse_response(
            r#"{
              "Errors": [],
              "SearchResults": {
                "NumberOfResult": 1,
                "Parts": [{
                  "MouserPartNumber": "356-ESP32C6W1N8",
                  "ManufacturerPartNumber": "ESP32-C6-WROOM-1-N8",
                  "Manufacturer": "Espressif Systems",
                  "Description": "WiFi Modules",
                  "Category": "WiFi Modules",
                  "Availability": "1250 In Stock",
                  "AvailabilityInStock": "1250",
                  "LifecycleStatus": "New Product",
                  "DataSheetUrl": "https://example.test/datasheet.pdf",
                  "ProductDetailUrl": "https://example.test/product",
                  "ROHSStatus": "ROHS3 Compliant",
                  "PriceBreaks": [{"Quantity": 1, "Price": "$3.40", "Currency": "USD"}],
                  "ProductAttributes": [{"AttributeName": "Supply Voltage", "AttributeValue": "3 V to 3.6 V"}]
                }]
              }
            }"#,
            None,
        )
        .unwrap();
        assert_eq!(response.total, 1);
        assert_eq!(
            response.parts[0].manufacturer_part_number,
            "ESP32-C6-WROOM-1-N8"
        );
        assert_eq!(response.parts[0].price_breaks[0].quantity, 1);
        assert_eq!(response.parts[0].attributes[0].name, "Supply Voltage");
    }

    #[test]
    fn surfaces_api_errors_without_echoing_a_key() {
        let secret = "f2f0-never-show-this";
        let body =
            format!(r#"{{"Errors":[{{"Code":"Invalid","Message":"API key {secret} rejected"}}]}}"#);
        let error = parse_response(&body, Some(secret)).unwrap_err().to_string();
        assert!(error.contains("Invalid: API key [redacted] rejected"));
        assert!(!error.contains(secret));
    }

    #[test]
    fn refuses_non_https_links_from_the_untrusted_response() {
        let error = parse_response(
            r#"{
              "Errors": [],
              "SearchResults": {
                "NumberOfResult": 1,
                "Parts": [{
                  "MouserPartNumber": "123-ABC",
                  "DataSheetUrl": "javascript:alert(1)"
                }]
              }
            }"#,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("non-HTTPS datasheet URL"));
        assert!(!error.contains("javascript"));
    }

    #[test]
    fn client_debug_redacts_the_key() {
        let client = MouserClient::new("secret-key").unwrap();
        let debug = format!("{client:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("secret-key"));
    }
}
