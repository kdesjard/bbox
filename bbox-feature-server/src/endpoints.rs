use crate::filter_params::FilterParams;
use crate::inventory::Inventory;
use crate::service::FeatureService;
use actix_web::{web, Error, HttpRequest, HttpResponse};
use bbox_core::api::OgcApiInventory;
use bbox_core::ogcapi::{ApiLink, CoreCollections};
#[cfg(feature = "stac")]
use bbox_core::ogcapi::{CoreFeature, CoreFeatures, STACCatalog};
use bbox_core::service::ServiceEndpoints;
use bbox_core::templates::{create_env_embedded, html_accepted, render_endpoint};
use minijinja::{context, Environment};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::error::Error as StdError;

/// the feature collections in the dataset
async fn collections(
    _ogcapi: web::Data<OgcApiInventory>,
    inventory: web::Data<Inventory>,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    let url = inventory.href_prefix();
    println!("{url}");
    let collections = CoreCollections {
        #[cfg(feature = "stac")]
        r#type: "Catalog".to_string(),
        links: vec![
            ApiLink {
                href: url.to_string(),
                rel: Some("root".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("landing page".to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
            ApiLink {
                href: format!("{url}/collections"),
                rel: Some("self".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("this document".to_string()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            },
        ],
        //TODO: include also collections from other services
        collections: inventory.collections(), //TODO: convert urls with absurl (?)
    };
    if html_accepted(&req).await {
        render_endpoint(
            &TEMPLATES,
            "collections.html",
            context!(cur_menu=>"Collections", base_url => inventory.base_url(), collections => &collections),
        )
        .await
    } else {
        Ok(HttpResponse::Ok().json(collections))
    }
}

/// describe the feature collection with id `collectionId`
async fn collection(
    inventory: web::Data<Inventory>,
    req: HttpRequest,
    collection_id: web::Path<String>,
) -> Result<HttpResponse, Error> {
    if let Some(collection) = inventory.core_collection(&collection_id) {
        if html_accepted(&req).await {
            render_endpoint(
                &TEMPLATES,
                "collection.html",
                context!(cur_menu=>"Collections", base_url => inventory.base_url(), collection => &collection),
            )
            .await
        } else {
            Ok(HttpResponse::Ok().json(collection))
        }
    } else {
        Ok(HttpResponse::NotFound().finish())
    }
}

/// describe the queryables available in the collection with id `collectionId`
async fn queryables(
    inventory: web::Data<Inventory>,
    req: HttpRequest,
    collection_id: web::Path<String>,
) -> Result<HttpResponse, Error> {
    if let Some(queryables) = inventory.collection_queryables(&collection_id).await {
        if html_accepted(&req).await {
            render_endpoint(
                &TEMPLATES,
                "queryables.html",
                context!(cur_menu=>"Collections", base_url => inventory.base_url(), queryables => &queryables),
            )
            .await
        } else {
            Ok(HttpResponse::Ok()
                .content_type("application/geo+json")
                .json(queryables))
        }
    } else {
        Ok(HttpResponse::NotFound().finish())
    }
}

/// fetch all features
#[cfg(feature = "stac")]
async fn search(inventory: web::Data<Inventory>, req: HttpRequest) -> Result<HttpResponse, Error> {
    let fp = match parse_query_params(&req) {
        Ok(filters) => filters,
        Err(e) => {
            log::error!("{e}");
            return Ok(HttpResponse::BadRequest().finish());
        }
    };

    let inventory_collections: Vec<String> = inventory
        .collections()
        .iter()
        .map(|c| c.id.clone())
        .collect();
    let collections = match fp.collections {
        Some(ref colls) => &colls.split(',').map(str::to_string).collect(),
        None => &inventory_collections,
    };
    let mut features: Vec<CoreFeature> = vec![];
    for collection in collections {
        if let Ok(Some(collection_features)) = inventory.collection_items(collection, &fp).await {
            features.extend(collection_features.features);
        } else {
            return Ok(HttpResponse::BadRequest().finish());
        }
    }
    let feature = CoreFeatures {
        type_: "FeatureCollection".to_string(),
        links: vec![],
        number_matched: Some(features.len() as u64),
        number_returned: Some(features.len() as u64),
        time_stamp: None,
        features,
    };
    Ok(HttpResponse::Ok()
        .content_type("application/geo+json")
        .json(feature))
}

fn parse_query_params(req: &HttpRequest) -> Result<FilterParams, Box<dyn StdError>> {
    let Ok(pairs) = serde_urlencoded::from_str::<Vec<(String, String)>>(req.query_string()) else {
        return Err("Bad".into());
    };
    let mut filters: HashMap<String, String> = pairs
        .iter()
        .filter_map(|(k, v)| {
            if k != "collections" || k != "ids" {
                Some((k.to_owned(), v.to_owned()))
            } else {
                None
            }
        })
        .collect();

    let bbox = filters.remove("bbox");
    let datetime = filters.remove("datetime");
    let collections = filters.remove("collections");
    let ids = filters.remove("ids");
    let intersects = filters.remove("intersects");
    if bbox.is_some() && intersects.is_some() {
        return Err("bbox and intersects are mutually exclusive options".into());
    }

    let offset = if let Some(offset_str) = filters.get("offset") {
        match offset_str.parse::<u64>() {
            Ok(o) => {
                filters.remove("offset");
                Some(o)
            }
            Err(e) => return Err(Box::new(e)),
        }
    } else {
        None
    };
    let limit = if let Some(limit_str) = filters.get("limit") {
        match limit_str.parse::<u64>() {
            Ok(o) => {
                filters.remove("limit");
                Some(o)
            }
            Err(e) => return Err(Box::new(e)),
        }
    } else {
        None
    };

    Ok(FilterParams {
        offset,
        limit,
        bbox,
        datetime,
        filters,
        collections,
        intersects,
        ids,
    })
}

/// fetch features
async fn features(
    inventory: web::Data<Inventory>,
    req: HttpRequest,
    collection_id: web::Path<String>,
) -> Result<HttpResponse, Error> {
    let fp = parse_query_params(&req)?;
    if let Some(collection) = inventory.core_collection(&collection_id) {
        if let Ok(Some(features)) = inventory.collection_items(&collection_id, &fp).await {
            if html_accepted(&req).await {
                render_endpoint(
                    &TEMPLATES,
                    "features.html",
                    context!(cur_menu=>"Collections", base_url => inventory.base_url(), collection => &collection, features => &features),
                ).await
            } else {
                Ok(HttpResponse::Ok()
                    .content_type("application/geo+json")
                    .json(features))
            }
        } else {
            Ok(HttpResponse::NotFound().finish())
        }
    } else {
        Ok(HttpResponse::NotFound().finish())
    }
}

/// fetch a single feature
async fn feature(
    inventory: web::Data<Inventory>,
    req: HttpRequest,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, Error> {
    let (collection_id, feature_id) = path.into_inner();
    if let Some(collection) = inventory.core_collection(&collection_id) {
        if let Some(feature) = inventory
            .collection_item(inventory.href_prefix(), &collection_id, &feature_id)
            .await
        {
            if html_accepted(&req).await {
                render_endpoint(
                    &TEMPLATES,
                    "feature.html",
                    context!(cur_menu=>"Collections", base_url => inventory.base_url(), collection => &collection, feature => &feature),
                ).await
            } else {
                Ok(HttpResponse::Ok()
                    .content_type("application/geo+json")
                    .json(feature))
            }
        } else {
            Ok(HttpResponse::NotFound().finish())
        }
    } else {
        Ok(HttpResponse::NotFound().finish())
    }
}

#[cfg(feature = "html")]
#[derive(rust_embed::RustEmbed)]
#[folder = "templates/"]
struct Templates;

#[cfg(not(feature = "html"))]
type Templates = bbox_core::templates::NoTemplates;

static TEMPLATES: Lazy<Environment<'static>> = Lazy::new(create_env_embedded::<Templates>);

impl ServiceEndpoints for FeatureService {
    fn register_endpoints(&self, cfg: &mut web::ServiceConfig) {
        cfg.app_data(web::Data::new(self.inventory.clone()))
            .service(web::resource("/collections").route(web::get().to(collections)))
            .service(web::resource("/collections.json").route(web::get().to(collections)))
            .service(
                web::resource("/collections/{collectionId}.json").route(web::get().to(collection)),
            )
            .service(web::resource("/collections/{collectionId}").route(web::get().to(collection)))
            .service(
                web::resource("/collections/{collectionId}/queryables.json")
                    .route(web::get().to(queryables)),
            )
            .service(
                web::resource("/collections/{collectionId}/queryables")
                    .route(web::get().to(queryables)),
            )
            .service(
                web::resource("/collections/{collectionId}/items").route(web::get().to(features)),
            )
            .service(
                web::resource("/collections/{collectionId}/items.json")
                    .route(web::get().to(features)),
            )
            .service(
                web::resource("/collections/{collectionId}/items/{featureId}.json")
                    .route(web::get().to(feature)),
            )
            .service(
                web::resource("/collections/{collectionId}/items/{featureId}")
                    .route(web::get().to(feature)),
            );
        #[cfg(feature = "stac")]
        cfg.service(web::resource("/catalog").route(web::get().to(catalog)))
            .service(web::resource("/catalog.json").route(web::get().to(catalog)))
            .service(web::resource("/search").route(web::get().to(search)));
    }
}

#[cfg(feature = "stac")]
async fn catalog(
    _ogcapi: web::Data<OgcApiInventory>,
    inventory: web::Data<Inventory>,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    let catalog_cfg = inventory.catalog();
    let url = inventory.href_prefix();
    let mut collection_links: Vec<ApiLink> = catalog_cfg
        .collections
        .iter()
        .filter_map(|c| {
            inventory.core_collection(c).map(|coll| ApiLink {
                href: format!("{url}/collections/{}", coll.id),
                rel: Some("child".to_string()),
                type_: Some("application/json".to_string()),
                title: None,
                hreflang: None,
                length: None,
                method: None,
            })
        })
        .collect();

    let mut catalog = STACCatalog {
        id: catalog_cfg.title.clone(),
        r#type: "Catalog".to_string(),
        title: Some(catalog_cfg.title),
        description: catalog_cfg.description,
        stac_version: "1.0.0".to_string(),
        stac_extensions: None,
        links: vec![
            ApiLink {
                href: format!("{url}/catalog.json"),
                rel: Some("root".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("this document".to_string()),
                hreflang: None,
                length: None,
                method: None,
            },
            ApiLink {
                href: format!("{url}/collections"),
                rel: Some("collections".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("collections".to_string()),
                hreflang: None,
                length: None,
                method: None,
            },
            ApiLink {
                href: format!("{url}/catalog.json"),
                rel: Some("self".to_string()),
                type_: Some("application/json".to_string()),
                title: Some("this document".to_string()),
                hreflang: None,
                length: None,
                method: None,
            },
        ],
    };
    catalog.links.append(&mut collection_links);
    if html_accepted(&req).await {
        render_endpoint(
            &TEMPLATES,
            "catalog.html",
            context!(cur_menu=>"Catalog", catalog => &catalog),
        )
        .await
    } else {
        Ok(HttpResponse::Ok().json(catalog))
    }
}
