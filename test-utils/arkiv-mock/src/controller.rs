use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Wrapper for JSON-serialized data that can be encoded and decoded.
#[derive(Debug, Clone, derive_more::Display)]
#[display("JSON object: {dtype}")]
pub struct JsonObject {
    dtype: String,
    data: String,
}

impl JsonObject {
    /// Create a JSON object from any serializable value.
    pub fn from<T: Serialize + DeserializeOwned>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(JsonObject {
            dtype: std::any::type_name::<T>().to_string(),
            data: serde_json::to_string(value)?,
        })
    }

    /// Deserialize the JSON data to a specific type.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_str(&self.data)
    }
}

/// Identifier for global overrides that apply to all RPC calls
const GLOBAL_OVERRIDE_KEY: &str = "global";

/// Result of waiting for an endpoint to be triggered
#[derive(Debug, Clone)]
pub enum CallbackResult {
    /// The endpoint was triggered and a response was sent
    Triggered,
    /// The notification channel was dropped
    ChannelDropped,
}

/// Wrapper object that provides an await function to wait for endpoint trigger
pub struct EndpointCallback {
    receiver: UnboundedReceiver<()>,
}

impl EndpointCallback {
    /// Wait for the endpoint to be triggered with a timeout
    #[must_use]
    pub async fn wait_for_trigger(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<CallbackResult, anyhow::Error> {
        match tokio::time::timeout(timeout, self.receiver.recv()).await {
            Ok(Some(())) => Ok(CallbackResult::Triggered),
            Ok(None) => Ok(CallbackResult::ChannelDropped),
            Err(_elapsed) => Err(anyhow::anyhow!(
                "Timeout {} waiting for endpoint",
                humantime::format_duration(timeout)
            )),
        }
    }

    /// Check if the callback was triggered within the timeout
    /// Returns an error if the callback wasn't triggered
    pub async fn triggered(&mut self, timeout: std::time::Duration) -> Result<(), anyhow::Error> {
        match self.wait_for_trigger(timeout).await? {
            CallbackResult::Triggered => Ok(()),
            CallbackResult::ChannelDropped => Err(anyhow::anyhow!("Callback channel was dropped")),
        }
    }
}

/// Generic wrapper for any response type that can include notification.
/// It sends a notification on drop.
#[derive(Debug, Clone)]
pub struct WithCallback<T: Display> {
    pub response: T,
    pub callback: Option<UnboundedSender<()>>,
    pub endpoint_name: String,
    pub call_count: usize,
}

impl<T: Display> WithCallback<T> {
    pub fn new(response: T, sender: UnboundedSender<()>, endpoint_name: String) -> Self {
        Self {
            response,
            callback: Some(sender),
            endpoint_name,
            call_count: 0,
        }
    }

    /// Increment the call count for this override
    pub fn increment_call_count(&mut self) {
        self.call_count += 1;
    }

    /// Clean up the callback to prevent unwanted message sending
    pub fn cleanup_callback(&mut self) {
        self.callback = None;
    }
}

impl<T: Display> Drop for WithCallback<T> {
    fn drop(&mut self) {
        // Send callback on drop to avoid missed callbacks in case of errors in
        // RCP handlers logic. If this would be done manually, developer could
        // forget to trigger it.
        if let Some(sender) = self.callback.take() {
            log::debug!(
                "{}: sending callback ({}-th time) for triggered response: {}",
                self.endpoint_name,
                self.call_count,
                self.response
            );

            if let Err(e) = sender.send(()) {
                log::warn!("Failed to send callback for {}: {}", self.endpoint_name, e);
            }
        }
    }
}

// Specific implementation for CallOverride types
impl WithCallback<CallOverride> {
    /// Check if this override is already outdated.
    pub fn should_remove_override(&self) -> bool {
        match &self.response {
            CallOverride::Once(_) => self.call_count >= 1, // Remove after first use
            CallOverride::Until { until, .. } => {
                // Remove if expired
                std::time::Instant::now() >= *until
            }
            CallOverride::NTimes { n, .. } => {
                // Remove if count exceeded
                self.call_count >= *n
            }
            CallOverride::Always(_) => false, // Always overrides are never outdated
        }
    }

    /// Check if this override should be used
    pub fn should_use_override(&self) -> bool {
        match &self.response {
            CallOverride::Once(_) => true,
            CallOverride::Until { until, .. } => std::time::Instant::now() < *until,
            CallOverride::NTimes { n, .. } => self.call_count < *n,
            CallOverride::Always(_) => true, // Always overrides are always used
        }
    }

    pub fn response(&self) -> &CallResponse {
        match &self.response {
            CallOverride::Once(response) => response,
            CallOverride::Until { response, .. } => response,
            CallOverride::NTimes { response, .. } => response,
            CallOverride::Always(response) => response,
        }
    }
}

/// Response types that can be forced for RPC calls.
#[derive(Debug, derive_more::Display, Clone)]
pub enum CallResponse {
    /// Return a specific error.
    /// TODO: We need to decide for a specific error type or make a wrapper that could handle multiple types.
    Error(String),
    /// Execute normal `subscribe_offer` logic (don't force any response).
    /// This variant allows to capture the fact of calling the RPC. User code can
    /// wait for this event to happen in test or validate this fact as a condition for test to pass.
    Success,
    /// RPC call will fail every Nth request based on the frequency value.
    /// For example, frequency 3 means every 3rd request fails.
    #[display("FailEachNth: {error} every {frequency} requests")]
    FailEachNth { error: String, frequency: usize },
    /// Return a custom non-error response with JSON-serialized data.
    #[display("Custom JSON response")]
    Custom(JsonObject),
}

impl CallResponse {
    /// Create a custom response from any JSON-serializable type.
    pub fn custom<T: Serialize + DeserializeOwned>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(CallResponse::Custom(JsonObject::from(value)?))
    }
}

#[derive(Debug, derive_more::Display, Clone)]
pub enum CallOverride {
    #[display("Override: once -> {}", _0)]
    Once(CallResponse),
    #[display("Override: until {until:?} -> {response}")]
    Until {
        response: CallResponse,
        until: std::time::Instant,
    },
    #[display("Override: {n} times -> {response}")]
    NTimes { response: CallResponse, n: usize },
    #[display("Override: always -> {}", _0)]
    Always(CallResponse),
}

/// Inner state of the mock controller
#[derive(Debug, Default)]
struct MockControllerInner {
    /// All overrides organized by key, with "global" having priority over RPC-specific keys.
    overrides: HashMap<String, Vec<WithCallback<CallOverride>>>,
}

impl MockControllerInner {
    /// Get the first valid override for a given key (RPC name or "global")
    /// Returns None if no valid override is found
    fn get_first_valid_override(&mut self, key: &str) -> Option<WithCallback<CallOverride>> {
        if let Some(queue) = self.overrides.get_mut(key) {
            if let Some(wrapper) = queue.first_mut() {
                if wrapper.should_use_override() {
                    wrapper.increment_call_count();
                    return Some(wrapper.clone());
                }
            }
        }
        None
    }

    /// Clean up all outdated entries from overrides
    fn cleanup_outdated_overrides(&mut self) {
        for queue in self.overrides.values_mut() {
            queue.retain_mut(|override_wrapper| {
                if override_wrapper.should_remove_override() {
                    // Clean up callback to prevent unwanted message sending
                    override_wrapper.cleanup_callback();
                    false // Remove this entry
                } else {
                    true // Keep this entry
                }
            });
        }
    }
}

/// Controller for managing mock market responses
#[derive(Debug, Default, Clone)]
pub struct MockController {
    inner: Arc<Mutex<MockControllerInner>>,
}

impl MockController {
    /// Create a new mock controller
    pub fn new() -> Self {
        Default::default()
    }

    /// Add a global override response that will be used for any RPC call
    /// Returns a notifier that can be used to wait for the endpoint to be triggered
    pub fn global_override(&self, rpc_override: CallOverride) -> EndpointCallback {
        log::debug!("Adding global override: {:?}", rpc_override);

        let (sender, receiver) = mpsc::unbounded_channel();
        let overrides = WithCallback::new(rpc_override, sender, GLOBAL_OVERRIDE_KEY.to_string());

        let mut lock = self.inner.lock().unwrap();
        lock.overrides
            .entry(GLOBAL_OVERRIDE_KEY.to_string())
            .or_insert_with(Vec::new)
            .push(overrides);

        EndpointCallback { receiver }
    }

    /// Add a response override for a specific RPC call
    /// Returns a notifier that can be used to wait for the endpoint to be triggered
    pub fn override_rpc(&self, rpc_name: &str, rpc_override: CallOverride) -> EndpointCallback {
        log::debug!("Adding RPC override for '{}': {:?}", rpc_name, rpc_override);

        let (sender, receiver) = mpsc::unbounded_channel();
        let overrides = WithCallback::new(rpc_override, sender, rpc_name.to_string());

        let mut lock = self.inner.lock().unwrap();
        lock.overrides
            .entry(rpc_name.to_string())
            .or_insert_with(Vec::new)
            .push(overrides);

        EndpointCallback { receiver }
    }

    /// Get the next override response for a specific RPC call (prioritizes global overrides)
    /// Handles Once, Until, and NTimes logic internally
    pub fn take_next_override(&self, rpc_name: &str) -> Option<WithCallback<CallOverride>> {
        let mut controller = self.inner.lock().unwrap();

        // First, clean up all outdated entries. This way in next step we will have
        // a list of overrides that are still valid and could be applied.
        controller.cleanup_outdated_overrides();

        // Global overrides have priority.
        if let Some(mut override_wrapper) = controller.get_first_valid_override(GLOBAL_OVERRIDE_KEY)
        {
            // User can know on which endpoint global override was triggered.
            override_wrapper.endpoint_name = format!("{GLOBAL_OVERRIDE_KEY}: {rpc_name}");
            controller.cleanup_outdated_overrides();
            return Some(override_wrapper);
        }

        // Then check RPC-specific overrides
        if let Some(override_wrapper) = controller.get_first_valid_override(rpc_name) {
            controller.cleanup_outdated_overrides();
            return Some(override_wrapper);
        }

        None
    }
}

/// Determines if a request should fail based on frequency
/// For frequency 3, every 3rd request fails
pub fn should_fail(frequency: usize, call_count: usize) -> bool {
    call_count > 0 && call_count % frequency == 0
}
