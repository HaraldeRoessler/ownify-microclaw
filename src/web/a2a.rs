// src/web/a2a.rs
// A2A web routes have been moved to the a2a-gateway.
// Microclaw's web API no longer serves A2A endpoints.
// The gateway handles all A2A protocol (JSON-RPC 2.0, Agent Card, Task Store).
// Microclaw only speaks internal REST with the gateway via /api/a2a/message
// (inbound from gateway) and /internal/outbound/<peer_did> (outbound to gateway).

// This file is kept as a module placeholder so the mod declaration in web.rs
// doesn't need to change. All functions have been removed.