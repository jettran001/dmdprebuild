use std::sync::Arc;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::processor::bridge_orchestrator::{BridgeOrchestrator, BridgeStatus, Chain};

// Đầu vào và đầu ra API

#[derive(Deserialize)]
pub struct RelayRequest {
    pub tx_hash: String,
}

#[derive(Serialize)]
pub struct BridgeStatusResponse {
    pub success: bool,
    pub tx_hash: String,
    pub status: Option<BridgeStatus>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct EstimateFeeParams {
    pub source_chain: Chain,
    pub target_chain: Chain,
    pub token_id: u64,
    pub amount: String,
}

#[derive(Serialize)]
pub struct FeesResponse {
    pub success: bool,
    pub source_chain: Chain,
    pub target_chain: Chain,
    pub fee_amount: Option<String>,
    pub fee_token: Option<String>,
    pub error: Option<String>,
}

// Hàm tạo router API
pub fn bridge_routes(orchestrator: Arc<BridgeOrchestrator>) -> Router {
    Router::new()
        .route("/bridge/status/:tx_hash", get(get_bridge_status))
        .route("/bridge/relay", post(relay_transaction))
        .route("/bridge/estimate-fee", get(estimate_fee))
        .with_state(orchestrator)
}

// Endpoint để kiểm tra trạng thái bridge
async fn get_bridge_status(
    State(orchestrator): State<Arc<BridgeOrchestrator>>,
    Path(tx_hash): Path<String>,
) -> Json<BridgeStatusResponse> {
    match orchestrator.get_transaction_status(&tx_hash).await {
        Ok(status) => Json(BridgeStatusResponse {
            success: true,
            tx_hash,
            status: Some(status),
            error: None,
        }),
        Err(e) => Json(BridgeStatusResponse {
            success: false,
            tx_hash,
            status: None,
            error: Some(e.to_string()),
        }),
    }
}

// Endpoint để thủ công relay một giao dịch 
async fn relay_transaction(
    State(orchestrator): State<Arc<BridgeOrchestrator>>,
    Json(payload): Json<RelayRequest>,
) -> (StatusCode, Json<BridgeStatusResponse>) {
    match orchestrator.get_transaction(&payload.tx_hash).await {
        Ok(tx) => {
            // Thử relay lại
            match orchestrator.validate_and_relay(tx).await {
                Ok(_) => (
                    StatusCode::OK,
                    Json(BridgeStatusResponse {
                        success: true,
                        tx_hash: payload.tx_hash,
                        status: Some(BridgeStatus::Pending),
                        error: None,
                    }),
                ),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(BridgeStatusResponse {
                        success: false,
                        tx_hash: payload.tx_hash,
                        status: None,
                        error: Some(e.to_string()),
                    }),
                ),
            }
        },
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(BridgeStatusResponse {
                success: false,
                tx_hash: payload.tx_hash,
                status: None,
                error: Some(e.to_string()),
            }),
        ),
    }
}

// Endpoint để ước tính phí bridge
async fn estimate_fee(
    State(orchestrator): State<Arc<BridgeOrchestrator>>,
    Query(params): Query<EstimateFeeParams>,
) -> Json<FeesResponse> {
    match orchestrator.estimate_bridge_fee(
        params.source_chain, 
        params.target_chain, 
        params.token_id, 
        params.amount
    ).await {
        Ok(fees) => Json(FeesResponse {
            success: true,
            source_chain: params.source_chain,
            target_chain: params.target_chain,
            fee_amount: Some(fees.fee_amount),
            fee_token: Some(fees.fee_token),
            error: None,
        }),
        Err(e) => Json(FeesResponse {
            success: false,
            source_chain: params.source_chain,
            target_chain: params.target_chain,
            fee_amount: None,
            fee_token: None,
            error: Some(e.to_string()),
        }),
    }
} 