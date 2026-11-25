use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    StreamDeckOriginal,
    StreamDeckMK2,
    StreamDeckMini,
    StreamDeckXL,
    StreamDeckPlus,
    StreamDeckPlusXL,
    StreamDeckPedal,
}

impl DeviceKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::StreamDeckOriginal => "Stream Deck (Original)",
            Self::StreamDeckMK2 => "Stream Deck MK.2",
            Self::StreamDeckMini => "Stream Deck Mini",
            Self::StreamDeckXL => "Stream Deck XL",
            Self::StreamDeckPlus => "Stream Deck +",
            Self::StreamDeckPlusXL => "Stream Deck + XL",
            Self::StreamDeckPedal => "Stream Deck Pedal",
        }
    }

    pub fn grid_dimensions(&self) -> (usize, usize) {
        match self {
            Self::StreamDeckMini => (2, 3),      // 2 rows, 3 cols = 6 keys
            Self::StreamDeckOriginal | Self::StreamDeckMK2 => (3, 5), // 3 rows, 5 cols = 15 keys
            Self::StreamDeckPlus => (2, 4),      // 2 rows, 4 cols = 8 keys (+ 4 dials)
            Self::StreamDeckPlusXL => (4, 9),    // 4 rows, 9 cols = 36 keys (+ 6 dials)
            Self::StreamDeckXL => (4, 8),        // 4 rows, 8 cols = 32 keys
            Self::StreamDeckPedal => (1, 3),     // 3 foot pedals
        }
    }

    pub fn total_keys(&self) -> usize {
        let (rows, cols) = self.grid_dimensions();
        rows * cols
    }

    pub fn has_dials(&self) -> bool {
        matches!(self, Self::StreamDeckPlus | Self::StreamDeckPlusXL)
    }

    pub fn dial_count(&self) -> usize {
        match self {
            Self::StreamDeckPlus => 4,
            Self::StreamDeckPlusXL => 6,
            _ => 0,
        }
    }
}
