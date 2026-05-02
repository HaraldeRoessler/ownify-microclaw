//! Egress (outbound) DLP hook. ownify-fork addition (Phase 3k v1).
//!
//! Outbound delivery passes every text message through an `EgressFilter`
//! before it reaches the channel adapter. The filter is meant to be wired
//! to ownify-egress-scanner (a separate cluster service) but the trait stays
//! transport-agnostic so upstream microclaw deployments can plug their own.
//!
//! Default behaviour when no filter is registered: pass-through. The hook
//! is fully transparent until a deployment opts in via
//! `ChannelRegistry::set_egress_filter`.
//!
//! Failure policy is decided by the concrete impl, not this trait. The
//! ownify-fork impl in `src/egress_scan.rs` defaults to fail-closed (refuse
//! send) when the scanner is configured but unreachable, so an outage of
//! the scanner doesn't silently disable DLP.

use async_trait::async_trait;
use std::sync::Arc;

/// Decision returned by an egress filter for a single outbound message.
#[derive(Debug, Clone)]
pub enum EgressDecision {
    /// Send `text` as the original adapter would.
    Allow,
    /// Send the substituted body — secrets have been replaced in-place.
    Redact { body: String },
    /// Do NOT send. The message contained content that policy refuses to
    /// transmit. The adapter returns Err to the caller; the agent loop will
    /// see the failure and (ideally) retry without the offending content.
    Refuse { reason: String },
}

#[async_trait]
pub trait EgressFilter: Send + Sync {
    /// Screen one outbound text payload bound for `channel_name`.
    async fn screen(&self, channel_name: &str, text: &str) -> EgressDecision;
}

/// Convenience type alias used by ChannelRegistry.
pub type EgressFilterArc = Arc<dyn EgressFilter>;
