//! Process manager: registry các `RunningSession` đang chạy (profile_id → pid/cdp_port),
//! giới hạn concurrency, theo dõi exit, cleanup khi crash/stop, emit event `profile://status`.
//!
//! Wave 2c implement.
