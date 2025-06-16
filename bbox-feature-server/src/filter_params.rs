use geozero::{geojson::GeoJson, ToGeo};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct FilterParams {
    // Pagination
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    // Filters
    pub bbox: Option<String>,
    pub datetime: Option<String>,
    pub filters: HashMap<String, String>,
    pub intersects: Option<String>,
    pub collections: Option<String>,
    pub ids: Option<String>,
}

#[derive(Debug)]
pub enum TemporalType {
    DateTime(chrono::DateTime<chrono::FixedOffset>),
    Open,
}

impl FilterParams {
    pub fn limit_or_default(&self) -> u64 {
        self.limit.unwrap_or(50)
    }
    pub fn with_offset(&self, offset: u64) -> FilterParams {
        let mut params = self.clone();
        params.offset = Some(offset);
        params
    }
    pub fn prev(&self) -> Option<FilterParams> {
        let offset = self.offset.unwrap_or(0);
        if offset > 0 {
            let prev = offset.saturating_sub(self.limit_or_default());
            Some(self.with_offset(prev))
        } else {
            None
        }
    }
    pub fn next(&self, max: u64) -> Option<FilterParams> {
        let offset = self.offset.unwrap_or(0);
        let next = offset.saturating_add(self.limit_or_default());
        if next < max {
            Some(self.with_offset(next))
        } else {
            None
        }
    }
    pub fn as_args(&self) -> String {
        let mut args = vec![
            Some("".to_string()),
            self.limit.map(|v| format!("limit={v}")),
            self.offset.map(|v| format!("offset={v}")),
            self.bbox.as_ref().map(|v| format!("bbox={v}")),
            self.datetime.as_ref().map(|v| format!("datetime={v}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<String>>()
        .join("&");

        for (key, val) in &self.filters {
            args.push_str(&format!("&{key}={val}"))
        }
        if !args.is_empty() {
            // replace & with ?
            args.replace_range(0..1, "?");
        }
        args
    }
    pub fn bbox(&self) -> Result<Option<Vec<f64>>, Box<dyn std::error::Error>> {
        if let Some(bboxstr) = &self.bbox {
            let bbox: Vec<f64> = bboxstr
                .split(',')
                .map(|v| v.parse())
                .collect::<Result<Vec<_>, _>>()?;
            if bbox.len() == 4 || bbox.len() == 6 {
                return Ok(Some(bbox));
            } else {
                return Err("Invalid length".into());
            }
        }
        Ok(None)
    }
    pub fn temporal(&self) -> Result<Option<Vec<TemporalType>>, Box<dyn std::error::Error>> {
        if let Some(dt) = &self.datetime {
            let parts: Vec<&str> = dt.split('/').collect();
            let mut parsed_parts = vec![];
            if parts.len() > 2 {
                return Err("Invalid datetimes".into());
            }
            let mut dts = vec![];
            for part in &parts {
                match *part {
                    ".." | "" => parsed_parts.push(TemporalType::Open),
                    p => {
                        let dt = chrono::DateTime::parse_from_rfc3339(p)?;
                        parsed_parts.push(TemporalType::DateTime(dt));
                        dts.push(dt);
                    }
                }
            }
            if dts.len() == 2 && dts[0] > dts[1] {
                return Err("Invalid datetimes".into());
            }
            return Ok(Some(parsed_parts));
        }
        Ok(None)
    }
    pub fn other_params(&self) -> Result<&HashMap<String, String>, Box<dyn std::error::Error>> {
        Ok(&self.filters)
    }
    pub fn ids(&self) -> Result<Option<Vec<String>>, Box<dyn std::error::Error>> {
        if let Some(ids) = &self.ids {
            let ids_vec: Vec<String> = ids.split(',').map(str::to_string).collect();
            return Ok(Some(ids_vec));
        };
        Ok(None)
    }
    pub fn intersects(&self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        if let Some(jsonstr) = &self.intersects {
            let geojson = GeoJson(jsonstr);
            // validate we have good GeoJSON
            let _ = geojson.to_geo()?;
            return Ok(Some(jsonstr.to_string()));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_to_args() {
        let filter = FilterParams {
            limit: Some(10),
            offset: Some(20),
            bbox: Some("1.0,2.2,3.33,4.444".to_string()),
            ..Default::default()
        };
        assert_eq!(
            filter.as_args(),
            "?limit=10&offset=20&bbox=1.0,2.2,3.33,4.444"
        );

        let filter = FilterParams {
            offset: Some(20),
            ..Default::default()
        };
        assert_eq!(filter.as_args(), "?offset=20");

        let filter = FilterParams::default();
        assert_eq!(filter.as_args(), "");

        let filter = FilterParams {
            datetime: Some("2024-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        assert_eq!(filter.as_args(), "?datetime=2024-01-01T00:00:00Z");

        let filter = FilterParams {
            filters: HashMap::from([("ArbitraryField".to_string(), "Something".to_string())]),
            ..Default::default()
        };
        assert_eq!(filter.as_args(), "?ArbitraryField=Something");
    }

    #[test]
    fn prev_next() {
        let filter = FilterParams {
            limit: Some(10),
            offset: Some(20),
            ..Default::default()
        };
        assert_eq!(filter.prev().unwrap().offset, Some(10));
        assert_eq!(filter.next(35).unwrap().offset, Some(30));
        assert!(filter.next(20).is_none());
        assert!(filter.next(19).is_none());

        let filter = FilterParams {
            limit: Some(10),
            offset: Some(10),
            ..Default::default()
        };
        assert_eq!(filter.prev().unwrap().offset, Some(0));
        assert_eq!(filter.next(35).unwrap().offset, Some(20));

        let filter = FilterParams {
            limit: Some(10),
            ..Default::default()
        };
        assert!(filter.prev().is_none());
        assert_eq!(filter.next(35).unwrap().offset, Some(10));
    }

    #[test]
    fn bbox_parse() {
        assert_eq!(
            FilterParams {
                bbox: Some("1.0,2.2,3.33,4.444".to_string()),
                ..Default::default()
            }
            .bbox()
            .unwrap(),
            Some(vec![1.0, 2.2, 3.33, 4.444])
        );

        assert_eq!(
            FilterParams {
                bbox: Some("1.0,2.2,3.33,4.444,5,6".to_string()),
                ..Default::default()
            }
            .bbox()
            .unwrap(),
            Some(vec![1.0, 2.2, 3.33, 4.444, 5.0, 6.0])
        );

        assert!(FilterParams {
            bbox: Some("1.0, 2.2, 3.33, 4.444".to_string()),
            ..Default::default()
        }
        .bbox()
        .is_err());

        assert!(FilterParams {
            bbox: Some("1,2,3".to_string()),
            ..Default::default()
        }
        .bbox()
        .is_err(),);
    }
}
