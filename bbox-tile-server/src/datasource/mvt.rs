use crate::config::TileDiagnosticsCfg;
use crate::datasource::TileSourceError;
use crate::service::QueryExtent;
use geozero::{mvt, mvt::Message, ToMvt};
use tile_grid::Xyz;

/// MVT tile builder helper.
pub struct MvtBuilder {
    tile: mvt::Tile,
}

impl MvtBuilder {
    pub fn new() -> Self {
        Self {
            tile: mvt::Tile::default(),
        }
    }
    pub fn new_layer(name: &str, tile_size: u32) -> MvtLayerBuilder {
        let mvt_layer = mvt::tile::Layer {
            version: 2,
            name: String::from(name),
            extent: Some(tile_size),
            ..Default::default()
        };
        MvtLayerBuilder {
            mvt_layer,
            tags: mvt::TagsBuilder::new(),
        }
    }
    pub fn push_layer(&mut self, layer: MvtLayerBuilder) {
        let mut mvt_layer = layer.mvt_layer;
        let (keys, values) = layer.tags.into_tags();
        mvt_layer.keys = keys;
        mvt_layer.values = values.into_iter().map(|v| v.into()).collect();
        self.tile.layers.push(mvt_layer);
    }
    pub fn into_blob(self) -> Result<Vec<u8>, TileSourceError> {
        let mut buf = Vec::new();
        self.tile
            .encode(&mut buf)
            .map_err(|_| TileSourceError::MvtEncodeError)?;
        Ok(buf)
    }
}

/// MVT layer builder helper.
pub struct MvtLayerBuilder {
    mvt_layer: mvt::tile::Layer,
    tags: mvt::TagsBuilder<String>,
}

impl MvtLayerBuilder {
    /// Add key/value to feature and layer tag collection
    pub fn add_feature_attribute(
        &mut self,
        mvt_feature: &mut mvt::tile::Feature,
        key: &str,
        mvt_value: mvt::tile::Value,
    ) -> Result<(), TileSourceError> {
        let (key_idx, val_idx) = self.tags.insert(
            key.to_string(),
            mvt_value
                .try_into()
                .map_err(|_| TileSourceError::MvtEncodeError)?,
        );
        mvt_feature.tags.push(key_idx);
        mvt_feature.tags.push(val_idx);
        Ok(())
    }
    pub fn push_feature(&mut self, mvt_feature: mvt::tile::Feature) {
        self.mvt_layer.features.push(mvt_feature);
    }
}

impl MvtBuilder {
    /// Diagnostics tile layer.
    pub fn add_diagnostics_layer(
        &mut self,
        cfg: &TileDiagnosticsCfg,
        tile: &Xyz,
        extent_info: &QueryExtent,
    ) -> Result<(), TileSourceError> {
        let extent = &extent_info.extent;
        const SIZE: u32 = 4096;
        const SIZE_F: f64 = 4096.0;
        let mut layer = MvtBuilder::new_layer("diagnostics-tile", SIZE);
        let geom: geo_types::Geometry<f64> = geo_types::Polygon::new(
            geo_types::LineString::from(vec![
                (0., 0.),
                (0., SIZE_F),
                (SIZE_F, SIZE_F),
                (SIZE_F, 0.),
                (0., 0.),
            ]),
            vec![],
        )
        .into();
        let mut feat = geom.to_mvt_unscaled()?;

        let mut layer_stats = self
            .tile
            .layers
            .iter()
            .map(|tl| {
                (
                    format!("layer-{}", &tl.name),
                    tl.encoded_len(),
                    tl.features.len(),
                )
            })
            .collect::<Vec<_>>();
        layer_stats.sort_by(|a, b| b.1.cmp(&a.1));

        layer.add_feature_attribute(
            &mut feat,
            "layer-total-bytes",
            mvt::TileValue::Uint(self.tile.encoded_len() as u64).into(),
        )?;
        let max_bytes = cfg.reference_size.unwrap_or(1_000_000); // 100% size 1MB uncompressed (compressed ~50%)
        layer.add_feature_attribute(
            &mut feat,
            "layer-total-percent",
            mvt::TileValue::Uint(100 * self.tile.encoded_len() as u64 / max_bytes).into(),
        )?;
        // Top 5 layers
        for tl in layer_stats.iter().take(5) {
            layer.add_feature_attribute(
                &mut feat,
                &format!("{}-bytes", tl.0),
                mvt::TileValue::Uint(tl.1 as u64).into(),
            )?;
            layer.add_feature_attribute(
                &mut feat,
                &format!("{}-count", tl.0),
                mvt::TileValue::Uint(tl.2 as u64).into(),
            )?;
        }

        layer.push_feature(feat);
        self.push_layer(layer);

        let mut layer = MvtBuilder::new_layer("diagnostics-label", SIZE);
        let geom: geo_types::Geometry<f64> = geo_types::Point::new(SIZE_F / 2., SIZE_F / 2.).into();
        let mut feat = geom.to_mvt_unscaled()?;
        layer.add_feature_attribute(
            &mut feat,
            "zxy",
            mvt::TileValue::Str(format!("{}/{}/{}", tile.z, tile.x, tile.y)).into(),
        )?;
        layer.add_feature_attribute(
            &mut feat,
            "tile-top",
            mvt::TileValue::Double(extent.top).into(),
        )?;
        layer.add_feature_attribute(
            &mut feat,
            "tile-left",
            mvt::TileValue::Double(extent.left).into(),
        )?;
        layer.add_feature_attribute(
            &mut feat,
            "tile-bottom",
            mvt::TileValue::Double(extent.bottom).into(),
        )?;
        layer.add_feature_attribute(
            &mut feat,
            "tile-right",
            mvt::TileValue::Double(extent.right).into(),
        )?;
        layer.push_feature(feat);
        self.push_layer(layer);

        Ok(())
    }
}
