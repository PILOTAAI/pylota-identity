//! Analytics Engine (binding `METRICS`). Writes never fail the request: a refused write is counted in
//! `metrics_dropped_total` (Observability § 3).

/// One `writeDataPoint`.
#[derive(Clone, Debug, PartialEq)]
pub struct DataPoint {
    pub indexes: [String; 1],
    pub blobs: Vec<String>,
    pub doubles: Vec<f64>,
}

pub trait Metrics {
    fn write(&self, point: DataPoint);
}
