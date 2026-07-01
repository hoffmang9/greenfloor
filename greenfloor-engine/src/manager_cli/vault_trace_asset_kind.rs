use crate::offer::VaultTraceAssetKind;
use crate::vault_coinset_scan::types::AssetTypeFilter;

impl VaultTraceAssetKind {
    #[must_use]
    pub fn json_label(self) -> &'static str {
        match self {
            Self::Xch => "xch",
            Self::Cat => "cat",
        }
    }

    #[must_use]
    pub fn scan_asset_type(self) -> AssetTypeFilter {
        match self {
            Self::Xch => AssetTypeFilter::Xch,
            Self::Cat => AssetTypeFilter::Cat,
        }
    }

    #[must_use]
    pub fn scan_cat_asset_id(self, asset_id: &str) -> Option<&str> {
        match self {
            Self::Cat => Some(asset_id),
            Self::Xch => None,
        }
    }
}
