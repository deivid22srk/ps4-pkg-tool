//! Núcleo do PKG Tool: leitura/análise de PKG do PS4, extração para pasta e
//! criação de PKG a partir de uma pasta.
//!
//! A leitura de PFS usa o crate `orbis-pfs` (MIT OR Apache-2.0). A escrita de
//! PFS/PKG foi implementada neste projeto a partir da documentação pública do
//! formato (psdevwiki) e do projeto LibOrbisPkg, usado apenas como referência
//! de formato.

pub mod error;
pub mod keys;
pub mod pack;
pub mod pfs_builder;
pub mod pkg;
pub mod progress;
pub mod sfo;

mod jni_bridge;

pub use error::{CoreError, Result};
pub use pack::{pack, PackOptions, PackSummary};
pub use pkg::{analyze, extract, AnalyzeInfo};
pub use progress::{is_cancelled, request_cancel as cancel, reset_cancel, NoopProgress, Progress};
