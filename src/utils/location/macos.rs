//! macOS Location API.
//!
//! Location sharing is not currently implemented on macOS. `is_available()`
//! returns `false`, which causes the application to disable the
//! location-sharing feature at the UI level.

use futures_util::{Stream, stream};
use geo_uri::GeoUri;

use super::{LocationError, LocationExt};

/// Location API under macOS.
#[derive(Debug, Default)]
pub(crate) struct MacOSLocation;

impl MacOSLocation {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl LocationExt for MacOSLocation {
    fn is_available(&self) -> bool {
        false
    }

    async fn init(&self) -> Result<(), LocationError> {
        Err(LocationError::Disabled)
    }

    async fn updates_stream(&self) -> Result<impl Stream<Item = GeoUri> + '_, LocationError> {
        Err::<stream::Empty<GeoUri>, _>(LocationError::Disabled)
    }
}
