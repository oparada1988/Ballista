use serde::{Deserialize, Serialize};

pub const ELGATO_VENDOR_ID: u16 = 0x0FD9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyImageFormat {
    Jpeg,
    Bmp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    StreamDeckOriginal,
    StreamDeckMK2,
    StreamDeckMini,
    StreamDeckXL,
    StreamDeckNeo,
    StreamDeckPlus,
    StreamDeckPlusXL,
    StreamDeckPedal,
    StreamDeckStudio,
}

impl DeviceKind {
    /// Maps an Elgato USB Product ID (VID 0x0FD9) to its DeviceKind and official model display name.
    /// Complete parity with abcminiuser/python-elgato-streamdeck ProductIDs.py & DeviceManager.py.
    pub fn from_usb_pid(pid: u16) -> Option<(Self, &'static str)> {
        match pid {
            0x0060 => Some((Self::StreamDeckOriginal, "Stream Deck (Original)")),
            0x0063 => Some((Self::StreamDeckMini, "Stream Deck Mini")),
            0x0090 => Some((Self::StreamDeckMini, "Stream Deck Mini (MK.2)")),
            0x00B3 => Some((Self::StreamDeckMini, "Stream Deck Mini (Discord)")),
            0x00B8 => Some((Self::StreamDeckMini, "Stream Deck Module 6")),
            0x006D => Some((Self::StreamDeckMK2, "Stream Deck (Original V2)")),
            0x0080 => Some((Self::StreamDeckMK2, "Stream Deck MK.2")),
            0x00A5 => Some((Self::StreamDeckMK2, "Stream Deck MK.2 (Scissor)")),
            0x00B9 => Some((Self::StreamDeckMK2, "Stream Deck MK.2 (V2 / Module 15)")),
            0x006C => Some((Self::StreamDeckXL, "Stream Deck XL")),
            0x008F => Some((Self::StreamDeckXL, "Stream Deck XL (V2)")),
            0x00BA => Some((Self::StreamDeckXL, "Stream Deck Module 32")),
            0x009A => Some((Self::StreamDeckNeo, "Stream Deck Neo")),
            0x0084 => Some((Self::StreamDeckPlus, "Stream Deck +")),
            0x00C6 => Some((Self::StreamDeckPlusXL, "Stream Deck + XL")),
            0x0086 => Some((Self::StreamDeckPedal, "Stream Deck Pedal")),
            0x00AA => Some((Self::StreamDeckStudio, "Stream Deck Studio")),
            _ => None,
        }
    }

    /// All supported Elgato USB Product IDs
    pub fn all_usb_pids() -> &'static [u16] {
        &[
            0x0060, 0x0063, 0x006C, 0x006D, 0x0080, 0x0084, 0x0086, 0x008F, 0x0090,
            0x009A, 0x00A5, 0x00AA, 0x00B3, 0x00B8, 0x00B9, 0x00BA, 0x00C6,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::StreamDeckOriginal => "Stream Deck (Original)",
            Self::StreamDeckMK2 => "Stream Deck MK.2",
            Self::StreamDeckMini => "Stream Deck Mini",
            Self::StreamDeckXL => "Stream Deck XL",
            Self::StreamDeckNeo => "Stream Deck Neo",
            Self::StreamDeckPlus => "Stream Deck +",
            Self::StreamDeckPlusXL => "Stream Deck + XL",
            Self::StreamDeckPedal => "Stream Deck Pedal",
            Self::StreamDeckStudio => "Stream Deck Studio",
        }
    }

    /// Returns `(rows, cols)` of the primary key matrix
    pub fn grid_dimensions(&self) -> (usize, usize) {
        match self {
            Self::StreamDeckMini => (2, 3),                           // 6 keys
            Self::StreamDeckOriginal | Self::StreamDeckMK2 => (3, 5), // 15 keys
            Self::StreamDeckNeo => (2, 4),                            // 8 LCD keys (+ info bar & 2 touch points)
            Self::StreamDeckPlus => (2, 4),                           // 8 keys (+ touchscreen + 4 dials)
            Self::StreamDeckPlusXL => (4, 9),                         // 36 keys (+ touchscreen + 6 dials)
            Self::StreamDeckXL => (4, 8),                             // 32 keys
            Self::StreamDeckPedal => (1, 3),                          // 3 foot pedals
            Self::StreamDeckStudio => (2, 16),                        // 32 keys (+ 2 rotary encoders)
        }
    }

    pub fn total_keys(&self) -> usize {
        let (rows, cols) = self.grid_dimensions();
        rows * cols
    }

    pub fn is_visual(&self) -> bool {
        !matches!(self, Self::StreamDeckPedal)
    }

    pub fn has_dials(&self) -> bool {
        matches!(
            self,
            Self::StreamDeckPlus | Self::StreamDeckPlusXL | Self::StreamDeckStudio
        )
    }

    pub fn dial_count(&self) -> usize {
        match self {
            Self::StreamDeckPlus => 4,
            Self::StreamDeckPlusXL => 6,
            Self::StreamDeckStudio => 2,
            _ => 0,
        }
    }

    pub fn has_touchscreen(&self) -> bool {
        matches!(self, Self::StreamDeckPlus | Self::StreamDeckPlusXL)
    }

    pub fn touchscreen_size(&self) -> Option<(u32, u32)> {
        match self {
            Self::StreamDeckPlus => Some((800, 100)),
            Self::StreamDeckPlusXL => Some((1200, 100)),
            _ => None,
        }
    }

    /// Counter-clockwise rotation in degrees required by hardware for touchscreen images
    pub fn touchscreen_rotation_ccw(&self) -> u16 {
        match self {
            Self::StreamDeckPlusXL => 90,
            _ => 0,
        }
    }

    pub fn has_info_screen(&self) -> bool {
        matches!(self, Self::StreamDeckNeo)
    }

    pub fn info_screen_size(&self) -> Option<(u32, u32)> {
        match self {
            Self::StreamDeckNeo => Some((248, 58)),
            _ => None,
        }
    }

    pub fn touch_key_count(&self) -> usize {
        match self {
            Self::StreamDeckNeo => 2,
            _ => 0,
        }
    }

    /// Native hardware LCD key pixel dimensions `(width, height)`
    pub fn key_image_size(&self) -> Option<(u32, u32)> {
        match self {
            Self::StreamDeckOriginal | Self::StreamDeckMK2 => Some((72, 72)),
            Self::StreamDeckMini => Some((80, 80)),
            Self::StreamDeckXL | Self::StreamDeckNeo => Some((96, 96)),
            Self::StreamDeckPlus => Some((120, 120)),
            Self::StreamDeckPlusXL => Some((112, 112)),
            Self::StreamDeckStudio => Some((80, 120)),
            Self::StreamDeckPedal => None,
        }
    }

    pub fn key_image_format(&self) -> KeyImageFormat {
        match self {
            Self::StreamDeckOriginal | Self::StreamDeckMini => KeyImageFormat::Bmp,
            _ => KeyImageFormat::Jpeg,
        }
    }

    /// Hardware image transform in PILHelper order: `(rotation_ccw_deg, flip_x, flip_y)`
    pub fn key_image_transform(&self) -> (u16, bool, bool) {
        match self {
            Self::StreamDeckOriginal
            | Self::StreamDeckMK2
            | Self::StreamDeckXL
            | Self::StreamDeckNeo => (0, true, true),
            Self::StreamDeckMini => (90, false, true),
            Self::StreamDeckPlusXL => (90, false, false),
            Self::StreamDeckPlus | Self::StreamDeckStudio | Self::StreamDeckPedal => {
                (0, false, false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_17_official_elgato_pids_are_recognized() {
        let pids = DeviceKind::all_usb_pids();
        assert_eq!(pids.len(), 17);
        for &pid in pids {
            assert!(
                DeviceKind::from_usb_pid(pid).is_some(),
                "PID {pid:#06x} missing from from_usb_pid"
            );
        }
        // Specific regression checks matching python-elgato-streamdeck ProductIDs.py
        assert_eq!(DeviceKind::from_usb_pid(0x00C6).unwrap().0, DeviceKind::StreamDeckPlusXL);
        assert_eq!(DeviceKind::from_usb_pid(0x0084).unwrap().0, DeviceKind::StreamDeckPlus);
        assert_eq!(DeviceKind::from_usb_pid(0x008F).unwrap().0, DeviceKind::StreamDeckXL);
        assert_eq!(DeviceKind::from_usb_pid(0x006C).unwrap().0, DeviceKind::StreamDeckXL);
        assert_eq!(DeviceKind::from_usb_pid(0x00BA).unwrap().0, DeviceKind::StreamDeckXL);
        assert_eq!(DeviceKind::from_usb_pid(0x0080).unwrap().0, DeviceKind::StreamDeckMK2);
        assert_eq!(DeviceKind::from_usb_pid(0x006D).unwrap().0, DeviceKind::StreamDeckMK2);
        assert_eq!(DeviceKind::from_usb_pid(0x00A5).unwrap().0, DeviceKind::StreamDeckMK2);
        assert_eq!(DeviceKind::from_usb_pid(0x00B9).unwrap().0, DeviceKind::StreamDeckMK2);
        assert_eq!(DeviceKind::from_usb_pid(0x0060).unwrap().0, DeviceKind::StreamDeckOriginal);
        assert_eq!(DeviceKind::from_usb_pid(0x0063).unwrap().0, DeviceKind::StreamDeckMini);
        assert_eq!(DeviceKind::from_usb_pid(0x0090).unwrap().0, DeviceKind::StreamDeckMini);
        assert_eq!(DeviceKind::from_usb_pid(0x00B3).unwrap().0, DeviceKind::StreamDeckMini);
        assert_eq!(DeviceKind::from_usb_pid(0x00B8).unwrap().0, DeviceKind::StreamDeckMini);
        assert_eq!(DeviceKind::from_usb_pid(0x009A).unwrap().0, DeviceKind::StreamDeckNeo);
        assert_eq!(DeviceKind::from_usb_pid(0x0086).unwrap().0, DeviceKind::StreamDeckPedal);
        assert_eq!(DeviceKind::from_usb_pid(0x00AA).unwrap().0, DeviceKind::StreamDeckStudio);
    }

    #[test]
    fn stream_deck_plus_xl_specs_match_python_elgato_streamdeck() {
        let d = DeviceKind::StreamDeckPlusXL;
        assert_eq!(d.grid_dimensions(), (4, 9));
        assert_eq!(d.total_keys(), 36);
        assert_eq!(d.dial_count(), 6);
        assert_eq!(d.key_image_size(), Some((112, 112)));
        assert_eq!(d.key_image_transform(), (90, false, false));
        assert_eq!(d.touchscreen_size(), Some((1200, 100)));
        assert_eq!(d.touchscreen_rotation_ccw(), 90);
        assert_eq!(d.key_image_format(), KeyImageFormat::Jpeg);
    }
}
