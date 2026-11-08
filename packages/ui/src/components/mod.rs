//! 共享组件模块。

pub mod editors;
pub mod graph_panel;
pub mod search_bar;

pub use editors::{
    DeckCraftPanel, EditorHost, GridCraftPanel, InsertTarget, RefInsertError, RefInsertPanel,
    WordCraftPanel, reference_insert_action,
};
pub use graph_panel::{GraphLegendV2, GraphPaneV2};
pub use search_bar::SearchBar;
