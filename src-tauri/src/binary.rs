//! Binary manager: tải binary CloakBrowser lúc runtime (reqwest + stream),
//! verify chữ ký Ed25519 + SHA-256, giải nén (tar/zip/flate2), emit event `binary://progress`.
//!
//! KHÔNG redistribute binary trong repo (Binary License). Wave 2b implement.
