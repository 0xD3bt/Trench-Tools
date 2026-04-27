use std::sync::OnceLock;

type OutboundProviderHttpRequestHook = fn();

fn outbound_provider_http_request_hook() -> &'static OnceLock<OutboundProviderHttpRequestHook> {
    static HOOK: OnceLock<OutboundProviderHttpRequestHook> = OnceLock::new();
    &HOOK
}

pub fn configure_outbound_provider_http_request_hook(hook: OutboundProviderHttpRequestHook) {
    let _ = outbound_provider_http_request_hook().set(hook);
}

pub fn record_outbound_provider_http_request() {
    if let Some(hook) = outbound_provider_http_request_hook().get().copied() {
        hook();
    }
}
