/// Trait for Kafka messaging service
use crate::security::input_validation::security;
use tokio::sync::Mutex as TokioMutex;
use tracing::{info, warn};
use tokio::sync::Notify;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use async_trait::async_trait;
use crate::infra::service_traits::ServiceError;

/// Maximum allowed Kafka message size in bytes
const MAX_KAFKA_PAYLOAD_SIZE: usize = 1024 * 1024; // 1MB
