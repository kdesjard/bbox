//! PostGIS feature source.

use crate::config::PostgisCollectionCfg;
#[cfg(feature = "stac")]
use crate::config::STACAssetCfg;
use crate::datasource::{
    AutoscanCollectionDatasource, CollectionDatasource, CollectionSource, CollectionSourceCfg,
    ConfiguredCollectionCfg, ItemsResult,
};
use crate::filter_params::{FilterParams, TemporalType};
use crate::inventory::FeatureCollection;
use async_trait::async_trait;
use bbox_core::error::{Error, Result};
use bbox_core::ogcapi::*;
use bbox_core::pg_ds::PgDatasource;
use chrono::DateTime;
use futures::TryStreamExt;
use log::{debug, error, info, warn};
use sqlx::postgres::{PgRow, PgTypeInfo};
use sqlx::{Column, Executor, Postgres, QueryBuilder, Row, Statement};
use std::collections::HashMap;

pub type Datasource = PgDatasource;

#[async_trait]
impl CollectionDatasource for PgDatasource {
    async fn setup_collection(
        &mut self,
        cfg: &ConfiguredCollectionCfg,
        base_url: &str,
        _extent: Option<CoreExtent>,
    ) -> Result<FeatureCollection> {
        info!("Setup Postgis Collection `{}`", &cfg.name);
        let CollectionSourceCfg::Postgis(ref srccfg) = cfg.source else {
            panic!();
        };

        let id = &cfg.name;
        if srccfg.table_name.is_none() && srccfg.sql.is_none() {
            return Err(Error::DatasourceSetupError(format!(
                "Datasource `{id}`: configuration `table_name` or `sql` missing"
            )));
        } else if srccfg.table_name.is_some() && srccfg.sql.is_some() {
            warn!("Datasource`{id}`: configuration `table_name` ignored, using `sql` instead");
        }
        let mut queryable_field_map = srccfg.queryable_field_mappings.clone();

        let (pk_column, geometry_column, sql) = if let Some(table_name) = &srccfg.table_name {
            let public = "public".to_string();
            let table_schema = srccfg.table_schema.as_ref().unwrap_or(&public);
            let pk_column = srccfg
                .fid_field
                .clone()
                .or(detect_pk(self, table_schema, table_name).await?);
            let geometry_column = detect_geometry(self, table_schema, table_name).await?;
            let sql = check_query(
                self,
                format!(r#"SELECT * FROM "{table_schema}"."{table_name}""#),
            )
            .await?;
            (pk_column, geometry_column, sql)
        } else {
            let pk_column = srccfg.fid_field.clone();
            // TODO: We should also allow user queries without geometry
            let geometry_field =
                srccfg
                    .geometry_field
                    .clone()
                    .ok_or(Error::DatasourceSetupError(format!(
                        "Datasource `{id}`: configuration `geometry_field` missing"
                    )))?;
            let geometry_column = if queryable_field_map.contains_key(&geometry_field) {
                queryable_field_map.remove(&geometry_field).unwrap()
            } else {
                geometry_field
            };
            let sql = check_query(self, srccfg.sql.clone().expect("config checked")).await?;
            (pk_column, geometry_column, sql)
        };
        if pk_column.is_none() {
            warn!("Datasource `{id}`: `fid_field` missing - single item queries will be ignored");
        }
        let queryables_types = get_column_info(self, &sql, &queryable_field_map).await?;
        let mut other_columns = HashMap::new();
        for (k, (v, t)) in &queryables_types {
            let queryable_type = match t.to_string().as_str() {
                "TEXT" | "VARCHAR" | "CHAR" => QueryableType::String,
                "INT4" | "INT8" => QueryableType::Integer,
                "FLOAT4" | "FLOAT8" => QueryableType::Number,
                "TIMESTAMP" | "TIMESTAMPTZ" => QueryableType::Datetime,
                "BOOL" => QueryableType::Bool,
                _ => {
                    return Err(Error::DatasourceSetupError(format!(
                        "{k} has a postgres type {v} which is not currently handled and can't be used a queryable"
                    )))
                }
            };
            other_columns.insert(k.clone(), (v.clone(), queryable_type));
        }
        #[cfg(feature = "stac")]
        let stac_asset_mappings = srccfg.stac_asset_mappings.clone().unwrap_or_default();

        let temporal_column = if let Some(tc) = &srccfg.temporal_field {
            if queryable_field_map.contains_key(tc) {
                queryable_field_map.remove(tc)
            } else {
                Some(tc.clone())
            }
        } else {
            None
        };
        let temporal_end_column = if let Some(tc) = &srccfg.temporal_end_field {
            if queryable_field_map.contains_key(tc) {
                queryable_field_map.remove(tc)
            } else {
                Some(tc.clone())
            }
        } else {
            None
        };

        let source = PgCollectionSource {
            ds: self.clone(),
            base_url: base_url.to_string(),
            sql,
            geometry_column_alias: srccfg.geometry_field.to_owned().ok_or(
                Error::DatasourceSetupError(format!(
                    "Datasource `{id}`: configuration `geometry_field` missing"
                )),
            )?,
            geometry_column,
            pk_column,
            temporal_column,
            temporal_end_column,
            ordering_column: srccfg.ordering_field.to_owned(),
            other_columns,
            max_results: srccfg.max_results,
            field_map: queryable_field_map.clone(),
            #[cfg(feature = "stac")]
            collection: id.to_string(),
            #[cfg(feature = "stac")]
            stac_asset_mappings,
        };

        let bbox = source
            .query_bbox()
            .await
            .unwrap_or(vec![-180.0, -90.0, 180.0, 90.0]);

        #[cfg(feature = "stac")]
        if srccfg.temporal_extents.is_none() {
            return Err(Error::DatasourceSetupError(
                "temporal_extents is a required configuration item for stac compliance".to_string(),
            ));
        }
        let temporal_extents: Option<CoreExtentTemporal> = match srccfg.temporal_extents.to_owned()
        {
            Some(t) => Some(
                t.try_into()
                    .map_err(|e: &str| Error::DatasourceSetupError(e.to_string()))?,
            ),
            None => None,
        };

        let mut collection = CoreCollection {
            id: id.clone(),
            title: Some(id.clone()),
            #[cfg(feature = "stac")]
            description: cfg.description.clone(),
            #[cfg(not(feature = "stac"))]
            description: Some(cfg.description.clone()),
            extent: Some(CoreExtent {
                spatial: Some(CoreExtentSpatial {
                    bbox: vec![bbox],
                    crs: None,
                }),
                temporal: temporal_extents,
            }),
            item_type: None,
            crs: vec![],
            links: vec![
                ApiLink {
                    href: base_url.to_string(),
                    rel: Some("root".to_string()),
                    type_: Some("application/geo+json".to_string()),
                    title: Some("The landing page of this server as JSON".to_string()),
                    hreflang: None,
                    length: None,
                    #[cfg(feature = "stac")]
                    method: None,
                },
                ApiLink {
                    #[cfg(feature = "stac")]
                    href: format!("{base_url}/catalog"),
                    #[cfg(not(feature = "stac"))]
                    href: format!("{base_url}/collections"),
                    rel: Some("parent".to_string()),
                    type_: Some("application/geo+json".to_string()),
                    title: Some("This document as JSON".to_string()),
                    hreflang: None,
                    length: None,
                    #[cfg(feature = "stac")]
                    method: None,
                },
                ApiLink {
                    href: format!("{base_url}/collections/{id}"),
                    rel: Some("self".to_string()),
                    type_: Some("application/geo+json".to_string()),
                    title: Some("This document as JSON".to_string()),
                    hreflang: None,
                    length: None,
                    #[cfg(feature = "stac")]
                    method: None,
                },
                ApiLink {
                    href: format!("{base_url}/collections/{id}/items"),
                    rel: Some("items".to_string()),
                    type_: Some("application/geo+json".to_string()),
                    title: Some(id.clone()),
                    hreflang: None,
                    length: None,
                    #[cfg(feature = "stac")]
                    method: None,
                },
            ],
            #[cfg(feature = "stac")]
            stac_version: "1.0.0".to_string(),
            #[cfg(feature = "stac")]
            stac_type: STACType::Collection,
            #[cfg(feature = "stac")]
            license: cfg.license.clone(),
        };

        if queryable_field_map.keys().len() > 0 {
            collection.links.push(ApiLink {
                href: format!("{base_url}/collections/{id}/queryables"),
                rel: Some("http://www.opengis.net/def/rel/ogc/1.0/queryables".to_string()),
                type_: Some("application/schema+json".to_string()),
                title: Some(id.clone()),
                hreflang: None,
                length: None,
                #[cfg(feature = "stac")]
                method: None,
            })
        }

        let fc = FeatureCollection {
            collection,
            source: Box::new(source),
        };
        Ok(fc)
    }
}

#[async_trait]
impl AutoscanCollectionDatasource for PgDatasource {
    async fn collections(&mut self, base_url: &str) -> Result<Vec<FeatureCollection>> {
        let mut collections = Vec::new();
        let sql = r#"
            SELECT contents.*
            FROM geometry_columns contents
              JOIN spatial_ref_sys refsys ON refsys.srid = contents.srid
            WHERE f_table_schema = 'public'
        "#;
        let mut rows = sqlx::query(sql).fetch(&self.pool);
        while let Some(row) = rows.try_next().await? {
            let table_schema: String = row.try_get("f_table_schema")?;
            let table_name: String = row.try_get("f_table_name")?;
            let coll_cfg = ConfiguredCollectionCfg {
                source: CollectionSourceCfg::Postgis(PostgisCollectionCfg {
                    table_schema: Some(table_schema),
                    table_name: Some(table_name.clone()),
                    ..Default::default()
                }),
                name: table_name.clone(),
                title: Some(table_name),
                description: String::new(),
                #[cfg(feature = "stac")]
                license: String::new(),
            };
            if let Ok(fc) = self.setup_collection(&coll_cfg, base_url, None).await {
                collections.push(fc);
            }
        }
        Ok(collections)
    }
}

#[derive(Clone, Debug)]
pub struct PgCollectionSource {
    ds: PgDatasource,
    #[allow(dead_code)]
    base_url: String,
    sql: String,
    geometry_column_alias: String,
    geometry_column: String,
    /// Primary key column, None if multi column key.
    pk_column: Option<String>,
    temporal_column: Option<String>,
    temporal_end_column: Option<String>,
    ordering_column: Option<String>,
    /// Queriable columns.
    other_columns: HashMap<String, (String, QueryableType)>,
    field_map: HashMap<String, String>,
    max_results: Option<u64>,
    #[cfg(feature = "stac")]
    collection: String,
    #[cfg(feature = "stac")]
    stac_asset_mappings: HashMap<String, STACAssetCfg>,
}

#[async_trait]
impl CollectionSource for PgCollectionSource {
    async fn items(&self, filter: &FilterParams) -> Result<ItemsResult> {
        let geometry_column = &self.geometry_column;
        let temporal_column = &self.temporal_column;
        let fid = &self.pk_column;
        if fid.is_none() && filter.ids.is_some() {
            return Err(Error::QueryParams);
        }
        let mut limit = filter.limit_or_default();
        let offset = filter.offset;
        let mut builder: QueryBuilder<Postgres> =
            QueryBuilder::new("WITH query AS (\n".to_string());

        builder.push(&self.sql);
        let mut where_term = self.sql.to_lowercase().contains("where");

        match filter.bbox() {
            Ok(Some(bbox)) => {
                if where_term {
                    builder.push(" AND ");
                } else {
                    builder.push(" WHERE ");
                }
                builder.push(format!(
                    " ( ST_Intersects({geometry_column}, ST_MakeEnvelope("
                ));
                let mut separated = builder.separated(",");
                separated.push_bind(bbox[0]);
                separated.push_bind(bbox[1]);
                separated.push_bind(bbox[2]);
                separated.push_bind(bbox[3]);
                builder.push(",4326) ) ) ");
                where_term = true;
            }
            Ok(None) => {}
            Err(e) => {
                error!("Ignoring invalid bbox: {e}");
                return Err(Error::QueryParams);
            }
        }
        match filter.ids() {
            Ok(Some(ids)) => {
                let fid = fid
                    .as_ref()
                    .expect("Already checked if is_some essentially");
                let actual_fid = self.field_map.get(fid).unwrap_or(fid);
                if where_term {
                    builder.push(" AND ");
                } else {
                    builder.push(" WHERE ");
                }
                builder.push(format!(" {actual_fid} in ("));
                let mut separated = builder.separated(",");
                for id in ids {
                    separated.push_bind(id);
                }
                builder.push(") ");
                where_term = true;
            }
            Ok(None) => {}
            Err(e) => {
                error!("Ignoring invalid bbox: {e}");
                return Err(Error::QueryParams);
            }
        }
        match filter.intersects() {
            Ok(Some(intersects)) => {
                if where_term {
                    builder.push(" AND ");
                } else {
                    builder.push(" WHERE ");
                }
                builder.push(format!(
                    " ST_Intersects({geometry_column},ST_GeomFromGeoJSON("
                ));
                builder.push_bind(intersects);
                builder.push(") ) ");
                where_term = true;
            }
            Ok(None) => {}
            Err(_) => {}
        }
        if let Some(temporal_column) = temporal_column {
            let temporal_end_column = self.temporal_end_column.as_ref().unwrap_or(temporal_column);
            match filter.temporal() {
                Ok(Some(parts)) => {
                    if where_term {
                        builder.push(" AND ");
                    } else {
                        builder.push(" WHERE ");
                        where_term = true;
                    }
                    if parts.len() == 1 {
                        if let TemporalType::DateTime(dt) = parts[0] {
                            builder.push(format!(" {temporal_column} = ",));
                            builder.push_bind(dt);
                            debug!("{temporal_column} = {dt}");
                        }
                    } else {
                        match parts[0] {
                            TemporalType::Open => match parts[1] {
                                TemporalType::Open => {
                                    error!("Open to Open datetimes doesn't make sense");
                                    return Err(Error::QueryParams);
                                }
                                TemporalType::DateTime(dt) => {
                                    builder.push(format!(" {temporal_column} <= ",));
                                    builder.push_bind(dt);
                                    debug!("{temporal_column} <= {dt}");
                                }
                            },
                            TemporalType::DateTime(dt1) => match parts[1] {
                                TemporalType::Open => {
                                    builder.push(format!(" {temporal_column} >= ",));
                                    builder.push_bind(dt1);
                                    debug!("{temporal_column} >= {dt1}");
                                }
                                TemporalType::DateTime(dt2) => {
                                    builder.push(format!(" {temporal_column} >= "));
                                    builder.push_bind(dt1);
                                    debug!("{temporal_column} >= {dt1}");
                                    builder.push(format!(" and {temporal_end_column} <= ",));
                                    builder.push_bind(dt2);
                                    debug!("{temporal_column} <= {dt2}");
                                }
                            },
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    error!("Ignoring invalid temporal field: {e}");
                    return Err(Error::QueryParams);
                }
            }
        }

        match filter.other_params() {
            Ok(others) => {
                if !others.is_empty() {
                    if where_term {
                        builder.push(" AND ");
                    } else {
                        builder.push(" WHERE ");
                    }
                }
                let mut separated = builder.separated(" AND ");
                for (key, val) in others {
                    // check if the passed in field matches queryables
                    // detect if value has wildcards
                    if let Some((k, v)) = self.other_columns.get_key_value(key) {
                        let (colname, coltype) = v;
                        if val.rfind('*').is_some() {
                            separated.push(format!("{colname}::text like "));
                            let val = val.replace('*', "%");
                            debug!("{k}::text like {val} - {coltype:#?}");
                            separated.push_bind_unseparated(val);
                        } else {
                            separated.push(format!("{colname}="));
                            debug!("{k} = {val} - {coltype:#?}");
                            match coltype {
                                QueryableType::String => separated.push_bind_unseparated(val),
                                QueryableType::Integer => separated.push_bind_unseparated(
                                    val.parse::<i64>().map_err(|_| Error::QueryParams)?,
                                ),
                                QueryableType::Number => separated.push_bind_unseparated(
                                    val.parse::<f64>().map_err(|_| Error::QueryParams)?,
                                ),
                                QueryableType::Bool => separated.push_bind_unseparated(
                                    val.parse::<bool>().map_err(|_| Error::QueryParams)?,
                                ),
                                QueryableType::Datetime => separated.push_bind_unseparated(
                                    DateTime::parse_from_rfc3339(val)
                                        .map_err(|_| Error::QueryParams)?,
                                ),
                            };
                        }
                    } else {
                        error!("Invalid query param {key}");
                        return Err(Error::QueryParams);
                    }
                }
            }
            Err(e) => {
                error!("{e}");
                return Err(Error::QueryParams);
            }
        }

        if let Some(ord) = &self.ordering_column {
            builder.push(format!(" ORDER BY {ord}"));
        }

        if let Some(max) = self.max_results {
            if limit > max {
                limit = max;
            }
            builder.push(format!(" LIMIT {limit}"));
            if offset.is_some() {
                builder.push(format!(" OFFSET {}", offset.unwrap()));
            }
        }
        builder.push(") ");
        let inner_sql = builder.sql();
        debug!("Inner SQL: {inner_sql}");
        let geometry_column_alias = &self.geometry_column_alias;
        let select_sql = if let Some(pk) = &self.pk_column {
            format!(
                r#"SELECT to_jsonb(t.*)-'{geometry_column_alias}'-'{pk}' AS properties, ST_AsGeoJSON({geometry_column_alias})::jsonb AS geometry, st_envelope({geometry_column_alias}::geometry) as bbox,
                    "{pk}"::varchar AS pk,
                      count(*) OVER () AS __total_cnt
                   FROM query t"#,
            )
        } else {
            format!(
                r#"SELECT to_jsonb(t.*)-'{geometry_column_alias}' AS properties, ST_AsGeoJSON({geometry_column_alias})::jsonb AS geometry, st_envelope({geometry_column_alias}::geometry) as bbox,
                      NULL AS pk,
                      --row_number() OVER () ::varchar AS pk,
                      count(*) OVER () AS __total_cnt
               FROM query t"#,
            )
        };
        builder.push(&select_sql);
        debug!("SQL: {}", builder.sql());
        let query = builder.build();
        let rows = query.fetch_all(&self.ds.pool).await?;
        let number_matched = match self.max_results {
            Some(_m) => None,
            None => Some(if let Some(row) = rows.first() {
                row.try_get::<i64, _>("__total_cnt")? as u64
            } else {
                0
            }),
        };
        let number_returned = rows.len() as u64;
        let items = rows
            .iter()
            .map(|row| row_to_feature(row, self))
            .collect::<Result<Vec<_>>>()?;
        let result = ItemsResult {
            features: items,
            number_matched,
            number_returned,
        };
        Ok(result)
    }

    async fn item(
        &self,
        _base_url: &str,
        collection_id: &str,
        feature_id: &str,
    ) -> Result<Option<CoreFeature>> {
        let Some(pk) = &self.pk_column else {
            warn!("Ignoring error getting item for {collection_id} without single primary key");
            return Ok(None);
        };
        let sql = format!(
            r#"
            WITH query AS ({sql})
            SELECT to_jsonb(t.*)-'{geometry_column_alias}'-'{pk}' AS properties, ST_AsGeoJSON({geometry_column_alias})::jsonb AS geometry, st_envelope({geometry_column_alias}::geometry) as bbox,
                "{pk}"::varchar AS pk
               FROM query t
               WHERE {pk}::varchar = '{feature_id}'"#,
            sql = &self.sql,
            geometry_column_alias = &self.geometry_column_alias,
        );
        debug!("{sql}");
        if let Some(row) = sqlx::query(&sql)
            // .bind(feature_id)
            .fetch_optional(&self.ds.pool)
            .await?
        {
            let item = row_to_feature(&row, self)?;
            Ok(Some(item))
        } else {
            Ok(None)
        }
    }
    async fn queryables(&self, collection_id: &str) -> Result<Option<Queryables>> {
        let properties: HashMap<String, QueryableProperty> = self
            .other_columns
            .iter()
            .map(|s| {
                let title = s.0.to_string();
                (
                    title.clone(),
                    QueryableProperty {
                        title: Some(title),
                        type_: Some(s.1 .1.clone()),
                        format: None,
                    },
                )
            })
            .collect();
        Ok(Some(Queryables {
            id: format!("/collections/{collection_id}/queryables"),
            title: Some(collection_id.to_string()),
            schema: "http://json-schema.org/draft/2019-09/schema".to_string(),
            type_: "object".to_string(),
            properties,
        }))
    }
}

#[allow(unused_variables)]
fn row_to_feature(row: &PgRow, colsrc: &PgCollectionSource) -> Result<CoreFeature> {
    let properties: serde_json::Value = row.try_get("properties")?;
    // properties[col.name()] = match col.type_info().name() {
    //     "VARCHAR"|"TEXT" => json!(row.try_get::<Option<&str>, _>(col.ordinal())?),
    //     "INT4" => json!(row.try_get::<Option<i32>, _>(col.ordinal())?),
    //     "INT8" => json!(row.try_get::<Option<i64>, _>(col.ordinal())?),
    //     "FLOAT4" => json!(row.try_get::<Option<f32>, _>(col.ordinal())?),
    //     "FLOAT8" => json!(row.try_get::<Option<f64>, _>(col.ordinal())?),
    //     ty => json!(format!("<{ty}>")),
    // }

    let geometry: serde_json::Value = row.try_get("geometry")?;

    #[cfg(feature = "stac")]
    let bbox_from_db: geozero::wkb::Decode<geo_types::Geometry<f64>> = row.try_get("bbox")?;
    #[cfg(feature = "stac")]
    let bbox = match bbox_from_db.geometry {
        Some(g) => {
            let polygon: geo_types::Polygon =
                g.try_into().map_err(|_| Error::GeometryFormatError)?;
            let exterior = polygon.into_inner().0;
            let points = exterior.into_points();
            vec![points[0].x(), points[0].y(), points[2].x(), points[2].y()]
        }
        None => vec![],
    };

    // ERROR:  lwgeom_to_geojson: 'CurvePolygon' geometry type not supported

    #[cfg(feature = "stac")]
    let assets: HashMap<String, STACAsset> = colsrc
        .stac_asset_mappings
        .iter()
        .map(|(k, v)| {
            (
                k.to_owned(),
                STACAsset {
                    href: properties[k].as_str().unwrap_or("Missing").to_string(),
                    title: v.title.clone(),
                    description: v.description.clone(),
                    roles: v.roles.clone(),
                    r#type: v.r#type.clone(),
                },
            )
        })
        .collect();

    let id: Option<String> = row.try_get("pk")?;

    #[allow(unused_mut)]
    let mut item = CoreFeature {
        type_: "Feature".to_string(),
        id: id.clone(),
        geometry,
        properties: Some(properties),
        links: vec![],
        #[cfg(feature = "stac")]
        stac_version: "1.0.0".to_string(),
        #[cfg(feature = "stac")]
        assets,
        #[cfg(feature = "stac")]
        collection: colsrc.collection.clone(),
        #[cfg(feature = "stac")]
        bbox,
    };
    #[cfg(feature = "stac")]
    {
        let url = &colsrc.base_url;
        let collection_id = &colsrc.collection;
        item.links = vec![
            ApiLink {
                href: url.clone(),
                rel: Some("root".to_string()),
                type_: Some("application/geo+json".to_string()),
                title: Some("The landing page of this server as JSON".to_string()),
                hreflang: None,
                length: None,
                method: None,
            },
            ApiLink {
                href: format!(
                    "{url}/collections/{collection_id}/items/{}",
                    id.clone().unwrap_or_default()
                ),
                rel: Some("self".to_string()),
                type_: Some("application/geo+json".to_string()),
                title: Some("this document".to_string()),
                hreflang: None,
                length: None,
                method: None,
            },
            ApiLink {
                href: format!("{url}/collections/{collection_id}"),
                rel: Some("collection".to_string()),
                type_: Some("application/geo+json".to_string()),
                title: Some("the collection document".to_string()),
                hreflang: None,
                length: None,
                method: None,
            },
            ApiLink {
                href: format!("{url}/collections/{collection_id}"),
                rel: Some("parent".to_string()),
                type_: Some("application/geo+json".to_string()),
                title: Some("the collection document".to_string()),
                hreflang: None,
                length: None,
                method: None,
            },
        ];
    }

    Ok(item)
}

impl PgCollectionSource {
    async fn query_bbox(&self) -> Result<Vec<f64>> {
        // TODO: Transform to WGS84, if necessary
        let sql = &format!(
            r#"
        WITH query AS ({sql}),
        extent AS (
          SELECT ST_Extent("{geometry_column}") AS bbox
          FROM query
        )
        SELECT ST_XMin(bbox), ST_YMin(bbox), ST_XMax(bbox), ST_YMax(bbox)
        FROM extent
    "#,
            sql = &self.sql,
            geometry_column = &self.geometry_column,
        );
        let row = sqlx::query(sql).fetch_one(&self.ds.pool).await?;
        let extent: Vec<f64> = vec![
            row.try_get(0)?,
            row.try_get(1)?,
            row.try_get(2)?,
            row.try_get(3)?,
        ];
        Ok(extent)
    }
}

async fn detect_pk(ds: &PgDatasource, schema: &str, table: &str) -> Result<Option<String>> {
    let sql = &format!(
        r#"
        WITH pkeys AS (
            SELECT a.attname
            FROM   pg_index i
            JOIN   pg_attribute a ON a.attrelid = i.indrelid
                                 AND a.attnum = ANY(i.indkey)
            WHERE  i.indrelid = '"{schema}"."{table}"'::regclass
            AND    i.indisprimary
        )
        SELECT
          (SELECT COUNT(*) FROM pkeys) AS pksize,
          (SELECT attname FROM pkeys LIMIT 1) AS pk
        "#
    );
    let row = sqlx::query(sql).fetch_one(&ds.pool).await?;
    let pksize: i64 = row.try_get("pksize")?;
    let pk_column: Option<String> = if pksize == 1 {
        row.try_get("pk")?
    } else {
        None
    };
    Ok(pk_column)
}

async fn detect_geometry(ds: &PgDatasource, schema: &str, table: &str) -> Result<String> {
    let sql = &format!(
        r#"
        SELECT f_geometry_column
        FROM geometry_columns
          JOIN spatial_ref_sys refsys ON refsys.srid = geometry_columns.srid
        WHERE f_table_schema = '{schema}' AND f_table_name = '{table}'
        "#
    );
    let row = sqlx::query(sql)
        // .bind(schema)
        // .bind(table)
        // We take the first result only
        .fetch_one(&ds.pool)
        .await?;
    let geometry_column: String = row.try_get("f_geometry_column")?;
    Ok(geometry_column)
}

async fn check_query(ds: &PgDatasource, sql: String) -> Result<String> {
    let mut limited_sql = sql.clone();
    limited_sql.push_str(" LIMIT 1");
    debug!("Collection query: {limited_sql}");
    if let Err(e) = ds.pool.acquire().await?.prepare(&limited_sql).await {
        error!("Error in collection query `{sql}`: {e}");
        return Err(e.into());
    }
    Ok(sql)
}

async fn get_column_info(
    ds: &PgDatasource,
    sql: &str,
    colmap: &HashMap<String, String>,
) -> Result<HashMap<String, (String, PgTypeInfo)>> {
    let mut limited_sql = sql.to_string();
    limited_sql.push_str(" LIMIT 1");
    match ds.pool.acquire().await?.prepare(&limited_sql).await {
        Ok(res) => {
            let mut tmphm = HashMap::new();
            for col in res.columns() {
                let colname = col.name().to_string();
                let type_info = col.type_info().clone();
                tmphm.insert(colname, type_info);
            }
            let mut hm = HashMap::new();
            for (k, v) in colmap {
                if let Some(t) = tmphm.remove(k) {
                    hm.insert(k.clone(), (v.clone(), t));
                }
            }
            Ok(hm)
        }
        Err(e) => {
            error!("Error in collection query `{sql}`: {e}");
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bbox_core::ogcapi::QueryableType;
    use std::collections::HashMap;
    use test_log::test;

    // docker run -p 127.0.0.1:5439:5432 -d --name mvtbenchdb --rm sourcepole/mvtbenchdb:v1.2

    #[test(tokio::test)]
    #[ignore]
    async fn pg_content() {
        let mut pool =
            PgDatasource::new_pool("postgresql://mvtbench:mvtbench@127.0.0.1:5439/mvtbench")
                .await
                .unwrap();
        let collections = pool.collections("").await.unwrap();
        assert!(collections.len() >= 3);
        assert!(collections
            .iter()
            .any(|col| col.collection.id == "ne_10m_rivers_lake_centerlines"));
    }

    #[test(tokio::test)]
    #[ignore]
    async fn pg_features() {
        let filter = FilterParams::default();
        let ds = PgDatasource::new_pool("postgresql://mvtbench:mvtbench@127.0.0.1:5439/mvtbench")
            .await
            .unwrap();
        let source = PgCollectionSource {
            ds,
            sql: "SELECT * FROM ne_10m_rivers_lake_centerlines".to_string(),
            geometry_column: "wkb_geometry".to_string(),
            geometry_column_alias: "wkb_geometry".to_string(),
            pk_column: Some("fid".to_string()),
            temporal_column: None,
            temporal_end_column: None,
            other_columns: HashMap::new(),
            base_url: String::new(),
            field_map: HashMap::new(),
            ordering_column: None,
            max_results: None,
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), filter.limit_or_default() as usize);
    }

    #[test(tokio::test)]
    #[ignore]
    async fn pg_bbox_filter() {
        let filter = FilterParams {
            limit: Some(50),
            offset: None,
            bbox: Some("633510.0904,5762740.4365,1220546.4677,6051366.6553".to_string()),
            // WGS84: 5.690918,45.890008,10.964355,47.665387
            ..Default::default()
        };
        let ds = PgDatasource::new_pool("postgresql://mvtbench:mvtbench@127.0.0.1:5439/mvtbench")
            .await
            .unwrap();
        let source = PgCollectionSource {
            ds,
            sql: "SELECT * FROM ne_10m_rivers_lake_centerlines".to_string(),
            geometry_column: "wkb_geometry".to_string(),
            geometry_column_alias: "wkb_geometry".to_string(),
            pk_column: Some("fid".to_string()),
            temporal_column: None,
            temporal_end_column: None,
            other_columns: HashMap::new(),
            base_url: String::new(),
            field_map: HashMap::new(),
            ordering_column: None,
            max_results: None,
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 10);
    }

    #[test(tokio::test)]
    #[ignore]
    async fn pg_datetime_filter() {
        let ds = PgDatasource::new_pool("postgresql://mvtbench:mvtbench@127.0.0.1:5439/mvtbench")
            .await
            .unwrap();
        let source = PgCollectionSource {
            ds,
            sql: "SELECT *, '2024-01-01 00:00:00Z'::timestamptz - (fid-1) * INTERVAL '1 day' AS ts FROM ne_10m_rivers_lake_centerlines ORDER BY fid".to_string(),
            geometry_column: "wkb_geometry".to_string(),
            geometry_column_alias: "wkb_geometry".to_string(),
            pk_column: Some("fid".to_string()),
            temporal_column: Some("ts".to_string()),
            temporal_end_column: None,
            other_columns: HashMap::new(),
            base_url: String::new(),
            field_map: HashMap::new(),
            ordering_column: None,
            max_results: None,
        };

        let filter = FilterParams {
            datetime: Some("2021-05-09T00:00:00Z".to_string()),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 1);

        // Combined with bbox
        let filter = FilterParams {
            bbox: Some("633510.0904,5762740.4365,1220546.4677,6051366.6553".to_string()),
            datetime: Some("2021-05-09T00:00:00Z".to_string()),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 1);

        // Outside of bbox
        let filter = FilterParams {
            bbox: Some("633510.0904,5762740.4365,1220546.4677,6051366.6553".to_string()),
            datetime: Some("2024-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 0);
    }

    #[test(tokio::test)]
    #[ignore]
    async fn pg_field_filter() {
        let ds = PgDatasource::new_pool("postgresql://mvtbench:mvtbench@127.0.0.1:5439/mvtbench")
            .await
            .unwrap();

        let other_columns: HashMap<String, (String, QueryableType)> = [(
            "name".to_string(),
            ("name".to_string(), QueryableType::String),
        )]
        .into();
        let source = PgCollectionSource {
            ds,
            sql: "SELECT *, '2024-01-01 00:00:00Z'::timestamptz - (fid-1) * INTERVAL '1 day' AS ts FROM ne_10m_rivers_lake_centerlines".to_string(),
            geometry_column: "wkb_geometry".to_string(),
            geometry_column_alias: "wkb_geometry".to_string(),
            pk_column: Some("fid".to_string()),
            temporal_column: Some("ts".to_string()),
            temporal_end_column: None,
            other_columns,
            base_url: String::new(),
            field_map: HashMap::new(),
            ordering_column: None,
            max_results: None,
        };

        let filter = FilterParams {
            filters: HashMap::from([("name".to_string(), "Rhein".to_string())]),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 2);

        // Existing filter column, but not queriable
        let filter = FilterParams {
            filters: HashMap::from([("scalerank".to_string(), "4".to_string())]),
            ..Default::default()
        };
        assert!(source.items(&filter).await.is_err());

        // Non-existing filter column
        let filter = FilterParams {
            filters: HashMap::from([("foo".to_string(), "bar".to_string())]),
            ..Default::default()
        };
        assert!(source.items(&filter).await.is_err());

        // Combined with bbox
        let filter = FilterParams {
            bbox: Some("633510.0904,5762740.4365,1220546.4677,6051366.6553".to_string()),
            // WGS84: 5.690918,45.890008,10.964355,47.665387
            filters: HashMap::from([("name".to_string(), "Rhein".to_string())]),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 2);

        // outside bbox
        let filter = FilterParams {
            bbox: Some("633510.0904,5762740.4365,633511,5762741".to_string()),
            filters: HashMap::from([("name".to_string(), "Rhein".to_string())]),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 0);

        // Combined with datetime
        let filter = FilterParams {
            datetime: Some("2021-05-09T00:00:00Z".to_string()),
            filters: HashMap::from([("name".to_string(), "Rhein".to_string())]),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 1);

        // Other datetime
        let filter = FilterParams {
            datetime: Some("2023-10-01T00:00:00Z".to_string()),
            filters: HashMap::from([("name".to_string(), "Rhein".to_string())]),
            ..Default::default()
        };
        let items = source.items(&filter).await.unwrap();
        assert_eq!(items.features.len(), 0);
    }
}
