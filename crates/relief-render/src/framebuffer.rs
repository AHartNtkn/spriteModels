use num_rational::Ratio;
use relief_core::CanonicalView;
use std::cmp::Ordering;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FragmentKey {
    pub depth: Ratio<i64>,
    pub chart_rank: u8,
    pub source_y: u32,
    pub source_x: u32,
}

impl Ord for FragmentKey {
    /// Orders by depth first, then the tie-break fields, identically to the
    /// derived `Ratio<i64>` ordering the predecessor used. `depth` is always a
    /// reduced canonical `Ratio<i64>` with positive denominator (produced by
    /// `num_rational` arithmetic), so comparing two depths by the sign of the
    /// cross-multiplication `n1*d2 - n2*d1` is exact and total: both products fit
    /// `i128` because each operand fits `i64` (`|n*d| <= 2^126`), and the shared
    /// positive-denominator normalization makes the sign the true value order.
    fn cmp(&self, other: &Self) -> Ordering {
        let left = i128::from(*self.depth.numer()) * i128::from(*other.depth.denom());
        let right = i128::from(*other.depth.numer()) * i128::from(*self.depth.denom());
        left.cmp(&right)
            .then_with(|| self.chart_rank.cmp(&other.chart_rank))
            .then_with(|| self.source_y.cmp(&other.source_y))
            .then_with(|| self.source_x.cmp(&other.source_x))
    }
}

impl PartialOrd for FragmentKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FragmentOwner {
    pub view: CanonicalView,
    pub depth: Ratio<i64>,
    pub source_y: u32,
    pub source_x: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameBuffer {
    width: u32,
    height: u32,
    pub(crate) keys: Vec<Option<FragmentKey>>,
    pub(crate) rgba: Vec<[u8; 4]>,
}

impl FrameBuffer {
    pub(crate) fn transparent(width: u32, height: u32) -> Self {
        let pixel_count = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            keys: vec![None; pixel_count],
            rgba: vec![[0, 0, 0, 0]; pixel_count],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[[u8; 4]] {
        &self.rgba
    }

    pub fn rgba_at(&self, x: u32, y: u32) -> [u8; 4] {
        self.rgba[(y * self.width + x) as usize]
    }

    pub fn owner_at(&self, x: u32, y: u32) -> Option<FragmentOwner> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let key = self.keys[(y * self.width + x) as usize].as_ref()?;
        Some(FragmentOwner {
            view: CanonicalView::from_rank(key.chart_rank)
                .expect("fragment keys only use canonical chart ranks"),
            depth: key.depth,
            source_y: key.source_y,
            source_x: key.source_x,
        })
    }
}

pub fn commit_fragment(frame: &mut FrameBuffer, x: u32, y: u32, key: FragmentKey, rgb: [u8; 3]) {
    let index = (y * frame.width() + x) as usize;
    if frame.keys[index]
        .as_ref()
        .is_none_or(|current| key < *current)
    {
        frame.keys[index] = Some(key);
        frame.rgba[index] = [rgb[0], rgb[1], rgb[2], 255];
    }
}
