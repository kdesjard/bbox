use bbox_core::config::{from_config_root_or_exit, ConfigError, DsPostgisCfg, NamedDatasourceCfg};
use bbox_core::service::ServiceConfig;
use clap::ArgMatches;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct FeatureServiceCfg {
    #[serde(rename = "datasource")]
    pub datasources: Vec<NamedDatasourceCfg>,
    #[serde(rename = "collections")]
    pub auto_collections: CollectionsCfg,
    #[serde(rename = "collection")]
    pub collections: Vec<ConfiguredCollectionCfg>,
    #[serde(rename = "catalog")]
    pub catalog: STACCatalogCfg,
}

/// Collections with auto-detection
#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct CollectionsCfg {
    pub directory: Vec<DsFiledirCfg>,
    pub postgis: Vec<DsPostgisCfg>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct DsFiledirCfg {
    pub dir: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredCollectionCfg {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    // extent: Option<CoreExtent>
    #[serde(flatten)]
    pub source: CollectionSourceCfg,
    #[cfg(feature = "stac")]
    pub license: String,
}

/// Collections with configuration
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub enum CollectionSourceCfg {
    #[serde(rename = "postgis")]
    Postgis(PostgisCollectionCfg),
    #[serde(rename = "gpkg")]
    Gpkg(GpkgCollectionCfg),
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct PostgisCollectionCfg {
    /// Name of datasource.postgis config (Default: first with matching type)
    pub datasource: Option<String>,
    // maybe we should allow direct DS URLs?
    // pub url: Option<String>,
    pub table_schema: Option<String>,
    pub table_name: Option<String>,
    /// Custom SQL query
    pub sql: Option<String>,
    pub fid_field: Option<String>,
    pub geometry_field: Option<String>,
    //pub field_list: Option<Vec<String>>,
    /// Field used for temporal filter expressions
    pub temporal_field: Option<String>,
    /// Field used for temporal end filter expressions
    pub temporal_end_field: Option<String>,
    /// Field used for ordering the results
    pub ordering_field: Option<String>,
    /// Fields which can be used in filter expressions
    #[serde(default)]
    pub queryable_field_mappings: HashMap<String, String>,
    pub max_results: Option<u64>, // per page?
    #[serde(skip_serializing_if = "Option::is_none")]
    // why NOT CoreExtentTemporal
    pub temporal_extents: Option<Vec<Vec<String>>>,
    #[cfg(feature = "stac")]
    pub stac_asset_mappings: Option<HashMap<String, STACAssetCfg>>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct GpkgCollectionCfg {
    /// Name of datasource.gpkg config (Default: first with matching type)
    pub datasource: Option<String>,
    pub table_name: Option<String>,
    /// Custom SQL query
    pub sql: Option<String>,
    pub fid_field: Option<String>,
    pub geometry_field: Option<String>,
    //pub field_list: Option<Vec<String>>,
}

/// Catalog
#[derive(Clone, Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct STACCatalogCfg {
    pub title: String,
    pub description: String,
    pub collections: Vec<String>,
}

#[derive(Deserialize, Default, Clone, Debug)]
pub struct STACAssetCfg {
    pub title: Option<String>,
    pub description: Option<String>,
    pub r#type: Option<String>,
    pub roles: Option<Vec<String>>,
}

impl ServiceConfig for FeatureServiceCfg {
    fn initialize(_cli: &ArgMatches) -> Result<Self, ConfigError> {
        let cfg: FeatureServiceCfg = from_config_root_or_exit();
        Ok(cfg)
    }
}

impl CollectionsCfg {
    #[allow(dead_code)]
    pub fn from_path(path: &str) -> Self {
        let mut cfg = CollectionsCfg::default();
        cfg.directory.push(DsFiledirCfg {
            dir: path.to_string(),
        });
        cfg
    }
}
