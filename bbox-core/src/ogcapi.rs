use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
/// <http://docs.opengeospatial.org/is/17-069r3/17-069r3.html#_api_landing_page>
pub struct CoreLandingPage {
    #[cfg(feature = "stac")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[cfg(feature = "stac")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[cfg(feature = "stac")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stac_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub links: Vec<ApiLink>,
    #[cfg(feature = "stac")]
    #[serde(flatten)]
    pub conforms_to: CoreConformsTo,
    pub extent: Option<CoreExtent>,
}

#[derive(Clone, Debug, Serialize)]
/// <http://schemas.opengis.net/ogcapi/features/part1/1.0/openapi/schemas/link.yaml>
pub struct ApiLink {
    pub href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hreflang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    #[cfg(feature = "stac")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// <http://docs.opengeospatial.org/is/17-069r3/17-069r3.html#_declaration_of_conformance_classes>
pub struct CoreConformsTo {
    pub conforms_to: Vec<String>,
}

#[derive(Debug, Serialize)]
/// /collections
/// <http://docs.opengeospatial.org/is/17-069r3/17-069r3.html#_collections_>
pub struct CoreCollections {
    #[cfg(feature = "stac")]
    pub r#type: String,
    pub links: Vec<ApiLink>,
    pub collections: Vec<CoreCollection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// /collections/{collectionId}.
/// <https://docs.opengeospatial.org/is/17-069r3/17-069r3.html#_collection_>
/// <http://schemas.opengis.net/ogcapi/features/part1/1.0/openapi/schemas/collection.yaml>
pub struct CoreCollection {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[cfg(feature = "stac")]
    pub description: String,
    #[cfg(not(feature = "stac"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub links: Vec<ApiLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<CoreExtent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub crs: Vec<String>,
    #[cfg(feature = "stac")]
    #[serde(rename = "type")]
    pub stac_type: STACType,
    #[cfg(feature = "stac")]
    #[serde(rename = "stac_version")]
    pub stac_version: String,
    #[cfg(feature = "stac")]
    pub license: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CoreExtent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spatial: Option<CoreExtentSpatial>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal: Option<CoreExtentTemporal>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CoreExtentSpatial {
    pub bbox: Vec<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoreExtentTemporal {
    pub interval: Vec<Vec<Option<String>>>, // date-time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trs: Option<String>,
}

// deal with the lack nulls in toml
impl TryFrom<Vec<Vec<String>>> for CoreExtentTemporal {
    type Error = &'static str;
    fn try_from(intervals: Vec<Vec<String>>) -> Result<Self, Self::Error> {
        let intervals: Result<Vec<Vec<Option<String>>>, &str> = intervals
            .iter()
            .map(|o| {
                o.iter()
                    .map(|i| {
                        if i.is_empty() {
                            Ok(None)
                        } else {
                            match DateTime::parse_from_rfc3339(i) {
                                Ok(_dt) => Ok(Some(i.to_string())),
                                Err(_) => Err("invalid datetime format"),
                            }
                        }
                    })
                    .collect()
            })
            .collect();
        Ok(CoreExtentTemporal {
            interval: intervals?,
            trs: None,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// /collections/{collectionId}/items
/// <https://docs.opengeospatial.org/is/17-069r3/17-069r3.html#_response_6>
pub struct CoreFeatures {
    // featureCollectionGeoJSON
    #[serde(rename = "type")]
    pub type_: String, // FeatureCollection
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<ApiLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_stamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_matched: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_returned: Option<u64>,
    pub features: Vec<CoreFeature>,
}

#[derive(Debug, Serialize)]
/// /collections/{collectionId}/items/{featureId}
/// <https://docs.opengeospatial.org/is/17-069r3/17-069r3.html#_feature_>
pub struct CoreFeature {
    #[serde(rename = "type")]
    pub type_: String, // Feature
    pub geometry: GeoJsonGeometry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<GeoJsonProperties>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>, // string or integer
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<ApiLink>,
    #[cfg(feature = "stac")]
    pub stac_version: String,
    #[cfg(feature = "stac")]
    pub collection: String,
    #[cfg(feature = "stac")]
    pub assets: HashMap<String, STACAsset>,
    #[cfg(feature = "stac")]
    pub bbox: Vec<f64>,
}

#[derive(Debug, Serialize)]
/// <https://docs.ogc.org/DRAFTS/19-079r1.html#queryables>
pub struct Queryables {
    #[serde(rename = "type")]
    pub type_: String, // Feature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "$id")]
    pub id: String,
    #[serde(rename = "$schema")]
    pub schema: String,
    pub properties: HashMap<String, QueryableProperty>,
}

#[derive(Debug, Serialize)]
/// <https://docs.ogc.org/DRAFTS/19-079r1.html#queryables>
pub struct QueryableProperty {
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<QueryableType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// <https://docs.ogc.org/DRAFTS/19-079r1.html#queryables>
pub enum QueryableType {
    #[serde(rename = "string")]
    String,
    #[serde(rename = "integer")]
    Integer,
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "boolean")]
    Bool,
    #[serde(rename = "datetime")]
    Datetime,
}

pub type GeoJsonProperties = serde_json::value::Value;
pub type GeoJsonGeometry = serde_json::value::Value;

#[cfg(feature = "stac")]
#[derive(Clone, Debug, Serialize)]
pub struct STACCatalog {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub description: String,
    pub stac_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stac_extensions: Option<Vec<String>>,
    pub links: Vec<ApiLink>,
}

#[cfg(feature = "stac")]
#[derive(Debug, Serialize)]
pub struct STACAsset {
    pub href: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub r#type: Option<String>,
    pub roles: Option<Vec<String>>,
}

#[cfg(feature = "stac")]
#[derive(Clone, Debug, Serialize)]
pub enum STACType {
    Catalog,
    Collection,
    Feature,
}
