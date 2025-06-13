use crate::config::FeatureServiceCfg;
use crate::datasource::Datasources;
use crate::inventory::Inventory;
use async_trait::async_trait;
use bbox_core::cli::{NoArgs, NoCommands};
use bbox_core::config::{error_exit, CoreServiceCfg};
use bbox_core::metrics::{no_metrics, NoMetrics};
use bbox_core::ogcapi::{ApiLink, CoreCollection};
use bbox_core::service::OgcApiService;

#[derive(Clone)]
pub struct FeatureService {
    pub inventory: Inventory,
}
#[async_trait]
impl OgcApiService for FeatureService {
    type Config = FeatureServiceCfg;
    type CliCommands = NoCommands;
    type CliArgs = NoArgs;
    type Metrics = NoMetrics;

    async fn create(config: &Self::Config, core_cfg: &CoreServiceCfg) -> Self {
        let mut sources = Datasources::create(&config.datasources)
            .await
            .unwrap_or_else(error_exit);

        let mut inventory =
            Inventory::scan(&config.auto_collections, core_cfg.public_server_url()).await;
        for cfg in &config.collections {
            let collection = sources
                .setup_collection(cfg, inventory.href_prefix())
                .await
                .unwrap_or_else(error_exit);
            inventory.add_collection(collection);
        }
        inventory.set_catalog(config.catalog.to_owned());
        FeatureService { inventory }
    }
    fn conformance_classes(&self) -> Vec<String> {
        let mut classes = vec![
            "http://www.opengis.net/spec/ogcapi-common-2/1.0/conf/collections".to_string(),
            "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/core".to_string(),
            "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/geojson".to_string(),
            "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/oas30".to_string(),
            // "http://www.opengis.net/spec/ogcapi-features-2/1.0/conf/crs".to_string(),
        ];
        if cfg!(feature = "html") {
            classes.extend(vec![
                "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/html".to_string(),
            ]);
        }
        if cfg!(feature = "stac") {
            classes.extend(vec![
                "https://api.stacspec.org/v1.0.0/core".to_string(),
                "https://api.stacspec.org/v1.0.0/ogcapi-features".to_string(),
                "https://api.stacspec.org/v1.0.0/item-search".to_string(),
            ]);
        }
        classes
    }
    fn landing_page_links(&self, api_base: &str) -> Vec<ApiLink> {
        #[allow(unused_mut)]
        let mut links = vec![
            #[cfg(feature = "stac")]
            ApiLink {
                href: api_base.to_string(),
                rel: Some("self".to_string()),
                type_: Some("application/json".to_string()),
                title: None,
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
            #[cfg(feature = "stac")]
            ApiLink {
                href: api_base.to_string(),
                rel: Some("root".to_string()),
                type_: Some("application/json".to_string()),
                title: None,
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
            #[cfg(feature = "stac")]
            ApiLink {
                href: format!("{api_base}/catalog"),
                rel: Some("child".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("Information about the catalog".to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
            ApiLink {
                href: format!("{api_base}/collections"),
                rel: Some("data".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("Information about the feature collections".to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
            ApiLink {
                href: format!("{api_base}/openapi.json"),
                rel: Some("service-desc".to_string()),
                type_: Some("application/json".to_string()),
                //type_: Some("application/vnd.oai.openapi+json;version=3.0".to_string()),
                title: Some("the API definition".to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
            ApiLink {
                href: format!("{api_base}/conformance"),
                rel: Some("conformance".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("OGC conformance classes implemented by this API".to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
            #[cfg(feature = "stac")]
            ApiLink {
                href: format!("{api_base}/search"),
                rel: Some("search".to_string()),
                type_: Some("application/geo+json".to_string()),
                title: Some("search".to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: Some("GET".to_string()),
            },
        ];
        #[cfg(feature = "stac")]
        for collection in self.inventory.collections() {
            links.push(ApiLink {
                href: format!("{api_base}/collections/{}", collection.id),
                rel: Some("child".to_string()),
                type_: Some("application/geo+json".to_string()),
                title: Some(collection.id),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            });
        }
        links
    }
    fn collections(&self) -> Vec<CoreCollection> {
        self.inventory.collections()
    }
    fn openapi_yaml(&self) -> Option<&str> {
        Some(include_str!("openapi.yaml"))
    }
    fn metrics(&self) -> &'static Self::Metrics {
        no_metrics()
    }
}
