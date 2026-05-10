const EXTENSION_RELOAD_FRIENDLY_MESSAGE =
  "Extension connection lost. Refresh to reconnect.";

function isExtensionContextInvalid(error) {
  const message = String(error?.message || "");
  return message.includes("Extension context invalidated")
    || message.includes("Receiving end does not exist");
}

function buildExtensionReloadError() {
  const failure = new Error(EXTENSION_RELOAD_FRIENDLY_MESSAGE);
  failure.code = "EXTENSION_RELOADED";
  failure.retryable = true;
  return failure;
}

export async function callBackground(type, payload) {
  let response;
  const startedAt = Date.now();
  try {
    response = await chrome.runtime.sendMessage({ type, payload });
  } catch (error) {
    if (isExtensionContextInvalid(error)) {
      throw buildExtensionReloadError();
    }
    throw error;
  }

  if (["trench:buy", "trench:sell", "trench:prime-trade-runtime"].includes(type)) {
    console.debug(
      "[trench][latency] phase=background-message type=%s clientRequestId=%s reason=%s roundtrip_ms=%s",
      type,
      payload?.clientRequestId || "",
      payload?.reason || "",
      Date.now() - startedAt
    );
  }

  if (!response?.ok) {
    const failure = new Error(response?.error || "Unknown extension error");
    if (response?.errorCode) {
      failure.code = response.errorCode;
    }
    if (Number.isInteger(response?.errorStatus)) {
      failure.status = response.errorStatus;
    }
    if (typeof response?.errorRetryable === "boolean") {
      failure.retryable = response.errorRetryable;
    }
    if (typeof response?.errorTimeout === "boolean") {
      failure.timeout = response.errorTimeout;
    }
    throw failure;
  }

  return response.data;
}
