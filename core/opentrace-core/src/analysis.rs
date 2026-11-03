//! Port of `analysis.Calibration`: manual scan -> bp anchors, interpolated only (never extrapolated).
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub anchors: Vec<(f64, f64)>,
    pub scans: Vec<f64>,
}

impl Calibration {
    pub fn new(anchors: &[(f64, f64)]) -> Result<Calibration, String> {
        let mut sorted: Vec<(f64, f64)> = anchors.to_vec();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
        if sorted.len() < 2 || sorted.iter().any(|&(x, y)| !x.is_finite() || !y.is_finite()) {
            return Err("At least two finite scan/bp anchors required".into());
        }
        if sorted.iter().any(|&(x, y)| x < 0.0 || y <= 0.0) {
            return Err("Scans must be nonnegative and bp positive".into());
        }
        if sorted.windows(2).any(|w| w[1].0 <= w[0].0 || w[1].1 <= w[0].1) {
            return Err("Anchor scans and bp must increase strictly".into());
        }
        let scans = sorted.iter().map(|a| a.0).collect();
        Ok(Calibration { anchors: sorted, scans })
    }

    /// Python `Calibration.size`: linear interpolation inside the anchor range, `None` outside it.
    pub fn size(&self, scan: f64) -> Option<f64> {
        if !(self.scans[0] <= scan && scan <= *self.scans.last().unwrap()) {
            return None;
        }
        let i = (self.scans.partition_point(|&x| x <= scan) as isize - 1).min(self.anchors.len() as isize - 2) as usize;
        let ((x0, y0), (x1, y1)) = (self.anchors[i], self.anchors[i + 1]);
        Some(y0 + (scan - x0) * (y1 - y0) / (x1 - x0))
    }
}
