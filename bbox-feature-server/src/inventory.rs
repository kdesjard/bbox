use crate::config::{CollectionsCfg, STACCatalogCfg};
use crate::datasource::{gpkg::SqliteDatasource, AutoscanCollectionDatasource, CollectionSource};
use crate::filter_params::FilterParams;
use bbox_core::error::Error;
use bbox_core::file_search;
use bbox_core::ogcapi::*;
use bbox_core::pg_ds::PgDatasource;
use chrono::Utc;
use log::{info, warn};
use std::collections::HashMap;

// ┌──────────────┐      ┌─────────────┐
// │              │1    n│             │
// │  Inventory   ├──────┤ Collection  │
// │              │      │             │
// └──────────────┘      └──────┬──────┘
//                              │n
//                              │
//                              │1
//                      ┌───────┴──────┐
//                      │  Datasource  │
//                      │              │
//                      │  (Pg, Gpkg)  │
//                      └──────────────┘

#[derive(Clone, Default)]
pub struct Inventory {
    // Key: collection_id
    feat_collections: HashMap<String, FeatureCollection>,
    base_url: String,
    catalog: STACCatalogCfg,
}

#[derive(Clone)]
/// Collection metadata with source specific infos like table name.
pub struct FeatureCollection {
    pub collection: CoreCollection,
    pub source: Box<dyn CollectionSource>,
}

impl Inventory {
    pub fn new(public_server_url: Option<String>) -> Self {
        let base_url = format!(
            "{}/",
            public_server_url
                .as_deref()
                .unwrap_or("")
                .trim_end_matches('/')
        );
        Inventory {
            feat_collections: HashMap::new(),
            base_url,
            catalog: STACCatalogCfg::default(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn href_prefix(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    pub async fn scan(config: &CollectionsCfg, public_server_url: Option<String>) -> Inventory {
        let mut inventory = Inventory::new(public_server_url);
        for dir_ds in &config.directory {
            let base_dir = &dir_ds.dir;
            info!("Scanning '{base_dir}' for feature collections");
            let files = file_search::search(base_dir, "*.gpkg");
            info!("Found {} matching file(s)", files.len());
            for path in files {
                let pathstr = path.as_os_str().to_string_lossy();
                match SqliteDatasource::new_pool(&pathstr).await {
                    Ok(mut ds) => {
                        info!("Scanning '{pathstr}' for feature collections");
                        match ds.collections(inventory.href_prefix()).await {
                            Ok(collections) => inventory.add_collections(collections),
                            Err(e) => {
                                warn!("Failed to scan feature collections for '{pathstr}': {e}")
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to create connection pool for '{pathstr}': {e}");
                        continue;
                    }
                }
            }
        }
        for cfg in &config.postgis {
            match PgDatasource::from_config(cfg, None).await {
                Ok(mut ds) => {
                    info!("Scanning '{}' for feature collections", cfg.url);
                    match ds.collections(inventory.href_prefix()).await {
                        Ok(collections) => inventory.add_collections(collections),
                        Err(e) => {
                            warn!("Failed to scan feature collections for '{}': {e}", &cfg.url)
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to create connection pool for '{}': {e}", &cfg.url);
                    continue;
                }
            }
        }
        // Close all connections, they will be reopened on demand
        // TODO: inventory.reset_pool().await.ok();
        inventory
    }

    pub fn add_collection(&mut self, fc: FeatureCollection) {
        let id = fc.collection.id.clone();
        // TODO: Handle name collisions
        self.feat_collections.insert(id, fc);
    }

    fn add_collections(&mut self, feat_collections: Vec<FeatureCollection>) {
        for fc in feat_collections {
            self.add_collection(fc);
        }
    }

    pub fn set_catalog(&mut self, cat: STACCatalogCfg) {
        self.catalog = cat;
    }

    /// Return all collections as vector
    pub fn collections(&self) -> Vec<CoreCollection> {
        self.feat_collections
            .values()
            .map(|fc| fc.collection.clone())
            .collect()
    }

    pub fn core_collection(&self, collection_id: &str) -> Option<&CoreCollection> {
        self.feat_collections
            .get(collection_id)
            .map(|fc| &fc.collection)
    }

    fn collection(&self, collection_id: &str) -> Option<&FeatureCollection> {
        self.feat_collections.get(collection_id)
    }

    pub async fn collection_items(
        &self,
        collection_id: &str,
        filter: &FilterParams,
    ) -> Result<Option<CoreFeatures>, Error> {
        let Some(fc) = self.collection(collection_id) else {
            warn!("Invalid collection {collection_id}");
            return Err(Error::InvalidCollection);
        };
        let items = match fc.source.items(filter).await {
            Ok(items) => items,
            Err(e) => {
                warn!("Ignoring error getting collection items for {collection_id}: {e}");
                return Err(e);
            }
        };
        let base_url = self.href_prefix();
        let mut features = CoreFeatures {
            type_: "FeatureCollection".to_string(),
            links: vec![
                ApiLink {
                    href: format!("{base_url}/"),
                    rel: Some("root".to_string()),
                    type_: Some("application/json".to_string()),
                    title: Some("The landing page of this server".to_string()),
                    hreflang: None,
                    length: None,
                    #[cfg(feature = "stac")]
                    method: None,
                },
                ApiLink {
                    href: format!("{base_url}/collections/{collection_id}"),
                    rel: Some("collection".to_string()),
                    type_: Some("application/geo+json".to_string()),
                    title: Some("the collection document".to_string()),
                    hreflang: None,
                    length: None,
                    #[cfg(feature = "stac")]
                    method: None,
                },
                ApiLink {
                    href: format!("{base_url}/collections/{collection_id}/items"),
                    rel: Some("self".to_string()),
                    type_: Some("application/geo+json".to_string()),
                    title: Some("this document".to_string()),
                    hreflang: None,
                    length: None,
                    #[cfg(feature = "stac")]
                    method: None,
                },
            ],
            time_stamp: Some(Utc::now()),
            number_matched: items.number_matched,
            number_returned: Some(items.number_returned),
            features: items.features,
        };
        let mut add_link = |link: FilterParams, rel: &str| {
            let params = link.as_args();
            features.links.push(ApiLink {
                href: format!("{base_url}/collections/{collection_id}/items{params}"),
                rel: Some(rel.to_string()),
                type_: Some("text/html".to_string()),
                title: Some(rel.to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            });
        };
        if items.number_matched > Some(items.number_returned) {
            if let Some(prev) = filter.prev() {
                add_link(prev, "prev");
            }
            if let Some(next) = filter.next(items.number_matched.unwrap()) {
                add_link(next, "next");
            }
        } else {
            let limit = filter.limit_or_default();
            let offset = filter.offset.unwrap_or(0);
            let limit = if limit > items.number_returned {
                items.number_returned
            } else {
                limit
            };
            if let Some(next) = filter.next(offset + limit + 1) {
                add_link(next, "next");
            }
        }
        Ok(Some(features))
    }

    pub async fn collection_item(
        &self,
        base_url: &str,
        collection_id: &str,
        feature_id: &str,
    ) -> Option<CoreFeature> {
        let Some(fc) = self.collection(collection_id) else {
            warn!("Ignoring error getting collection {collection_id}");
            return None;
        };
        match fc.source.item(base_url, collection_id, feature_id).await {
            Ok(item) => item,
            Err(e) => {
                warn!("Ignoring error getting collection item for {collection_id}: {e}");
                None
            }
        }
    }

    pub async fn collection_queryables(&self, collection_id: &str) -> Option<Queryables> {
        let Some(fc) = self.collection(collection_id) else {
            warn!("Ignoring error getting collection {collection_id}");
            return None;
        };
        match fc.source.queryables(collection_id).await {
            Ok(queryables) => queryables,
            Err(e) => {
                warn!("Ignoring error getting collection items for {collection_id}: {e}");
                None
            }
        }
    }

    /// Return all collections as vector
    pub fn catalog(&self) -> STACCatalogCfg {
        self.catalog.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inventory_scan() {
        let inventory = Inventory::scan(&CollectionsCfg::from_path("../assets"), None).await;
        // assert_eq!(inventory.collections().len(), 3);
        assert!(inventory.collections().len() >= 3);
        assert_eq!(
            inventory
                .core_collection("ne_10m_lakes")
                .map(|col| col.id.clone()),
            Some("ne_10m_lakes".to_string())
        );
    }
}
