//! How much each model stores: preset slots and cabinet slots (spec
//! section 4.6).

use axegrid_sysex::Model;

/// Preset slots of a model, numbered from 0.
pub fn preset_count(model: Model) -> u16 {
    match model {
        Model::AxeFx2 => 384,
        Model::AxeFx2Xl | Model::AxeFx2XlPlus => 768,
    }
}

/// Where the user cabs sit in a model's cab list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CabSlots {
    /// Factory cabs, which come first in the list.
    pub factory: u16,
    /// User cabs, which follow them.
    pub user: u16,
}

impl CabSlots {
    /// Position in the cab list of a user cab numbered from 1.
    pub fn list_index(self, cab: u16) -> Option<u16> {
        (1..=self.user).contains(&cab).then(|| self.factory + cab - 1)
    }
}

/// The cab memory of a model, where it is known.
pub fn cab_slots(model: Model) -> Option<CabSlots> {
    match model {
        // Read from an XL; the XL+ is documented with the same memory.
        Model::AxeFx2Xl | Model::AxeFx2XlPlus => Some(CabSlots { factory: 189, user: 1024 }),
        // The number of factory cabs on the Mark I/II is not known here.
        Model::AxeFx2 => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_cabs_follow_the_factory_cabs() {
        let slots = cab_slots(Model::AxeFx2Xl).unwrap();
        assert_eq!(slots.list_index(1), Some(189));
        assert_eq!(slots.list_index(1024), Some(1212));
        assert_eq!((slots.list_index(0), slots.list_index(1025)), (None, None));
        assert!(cab_slots(Model::AxeFx2).is_none());
        assert_eq!((preset_count(Model::AxeFx2), preset_count(Model::AxeFx2XlPlus)), (384, 768));
    }
}
