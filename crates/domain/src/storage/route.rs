use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriveMedium {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractionRoute {
    DirectToTarget,
    CacheInputThenExtract,
    CacheExtractThenMove,
}

pub fn decide_route(
    cache_enabled: bool,
    source_medium: DriveMedium,
    target_medium: DriveMedium,
    cache_input_pref: bool,
) -> ExtractionRoute {
    if !cache_enabled {
        return ExtractionRoute::DirectToTarget;
    }
    match (source_medium, target_medium) {
        (DriveMedium::Hdd, DriveMedium::Hdd) => {
            if cache_input_pref {
                ExtractionRoute::CacheInputThenExtract
            } else {
                ExtractionRoute::CacheExtractThenMove
            }
        }
        (DriveMedium::Hdd, DriveMedium::Ssd) => {
            if cache_input_pref {
                ExtractionRoute::CacheInputThenExtract
            } else {
                ExtractionRoute::DirectToTarget
            }
        }
        _ => ExtractionRoute::DirectToTarget,
    }
}
